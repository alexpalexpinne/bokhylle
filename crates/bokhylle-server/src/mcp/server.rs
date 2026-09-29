//! MCP tools. Each one resolves the agent principal from the
//! request extensions and calls the shared application services; no product
//! logic lives here.

use rmcp::{
    ErrorData as McpError, Json, RoleServer, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ServerCapabilities, ServerConfig},
    service::RequestContext,
    tool, tool_handler, tool_router,
};

use crate::AppState;
use crate::agent_tokens::AgentPrincipal;
use crate::error::AppError;
use crate::library::queries;
use crate::services;

use super::types::{
    AddBookInput, AddCatalogueBookInput, BookOutput, CatalogueAcquisitionOutput, CatalogueBook,
    ContinueItem, ContinueOutput, DeliveryOutput, GetBookInput, IntentOutput, LibraryBook,
    ProgressOutput, RequestBookInput, RequestOutput, RequestsOutput, SearchBooksInput,
    SearchBooksOutput, SendToReaderInput, ShelfInput, ShelfOutput,
};

#[derive(Clone)]
pub struct BokhylleMcp {
    state: AppState,
    tool_router: ToolRouter<Self>,
}

fn tool_error(error: AppError) -> McpError {
    if error.status().is_server_error() {
        McpError::internal_error(error.to_string(), None)
    } else {
        McpError::invalid_request(error.to_string(), None)
    }
}

fn principal(ctx: &RequestContext<RoleServer>) -> Result<AgentPrincipal, McpError> {
    ctx.extensions
        .get::<axum::http::request::Parts>()
        .and_then(|parts| parts.extensions.get::<AgentPrincipal>())
        .cloned()
        .ok_or_else(|| McpError::internal_error("missing agent principal", None))
}

fn require_write(principal: &AgentPrincipal) -> Result<(), McpError> {
    if principal.can_write() {
        Ok(())
    } else {
        Err(McpError::invalid_request(
            "this token is read-only; use a read & write token",
            None,
        ))
    }
}

fn library_book(book: queries::BookSummary) -> LibraryBook {
    LibraryBook {
        id: book.id,
        title: book.title,
        authors: book.authors,
        language: book.language,
        series: book.series,
        series_number: book.series_number,
    }
}

fn request_output(request: crate::book_requests::BookRequestView) -> RequestOutput {
    RequestOutput {
        id: request.id,
        book_id: request.book_id,
        title: request.title,
        authors: request.authors,
        requester: request.requester,
        status: request.status,
        phase: request.phase,
        acquisition_id: request.acquisition_id,
        error_code: request.error_code,
        created_at: request.created_at,
    }
}

#[tool_router]
impl BokhylleMcp {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Search the owned library or the public metadata catalogue. scope: library (owned books; children see only their shelf), catalogue, or auto (library first, catalogue as a fallback)."
    )]
    async fn search_books(
        &self,
        Parameters(input): Parameters<SearchBooksInput>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<Json<SearchBooksOutput>, McpError> {
        let principal = principal(&ctx)?;
        let scope = services::books::SearchScope::parse(input.scope.as_deref().unwrap_or("auto"))
            .ok_or_else(|| {
            McpError::invalid_params("scope must be library, catalogue or auto", None)
        })?;
        let outcome = services::books::search(
            &self.state,
            &principal.user,
            &input.query,
            scope,
            input.limit.unwrap_or(20),
        )
        .await
        .map_err(tool_error)?;
        Ok(Json(SearchBooksOutput {
            library: outcome.library.into_iter().map(library_book).collect(),
            catalogue: outcome
                .catalogue
                .into_iter()
                .map(|hit| CatalogueBook {
                    provider: hit.provider,
                    provider_key: hit.provider_key,
                    title: hit.title,
                    authors: hit.authors,
                    year: hit.year,
                    language: hit.language,
                    series: hit.series,
                    series_number: hit.series_number,
                })
                .collect(),
        }))
    }

    #[tool(
        description = "Get one book's details, ownership and the profile's reading progress. Children can only open books on their shelf."
    )]
    async fn get_book(
        &self,
        Parameters(input): Parameters<GetBookInput>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<Json<BookOutput>, McpError> {
        let principal = principal(&ctx)?;
        let Some(view) = services::books::get(&self.state, &principal.user, input.book_id)
            .await
            .map_err(tool_error)?
        else {
            return Err(McpError::invalid_params("book not found", None));
        };
        Ok(Json(BookOutput {
            id: view.book.id,
            title: view.book.title,
            authors: view.book.authors,
            language: view.book.language,
            series: view.book.series,
            series_number: view.book.series_number,
            description: view.book.description,
            publication_year: view.book.publication_year,
            on_shelf: view.book.on_shelf,
            preference: view.book.preference,
            available: !view.book.files.is_empty(),
            progress: view.progress.map(|progress| ProgressOutput {
                percentage: progress.percentage,
                updated_at: progress.updated_at,
            }),
        }))
    }

    #[tool(description = "List the profile's own shelf, most recently added first.")]
    async fn list_my_shelf(
        &self,
        Parameters(input): Parameters<ShelfInput>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<Json<ShelfOutput>, McpError> {
        let principal = principal(&ctx)?;
        let books = queries::recent_books(
            &self.state.db,
            input.limit.unwrap_or(24),
            Some(principal.user.id),
        )
        .await
        .map_err(tool_error)?;
        Ok(Json(ShelfOutput {
            items: books.into_iter().map(library_book).collect(),
        }))
    }

    #[tool(description = "Books the profile has started and not finished, newest first.")]
    async fn continue_reading(
        &self,
        Parameters(input): Parameters<ShelfInput>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<Json<ContinueOutput>, McpError> {
        let principal = principal(&ctx)?;
        let child = crate::auth::profile_type(&self.state.db, principal.user.id)
            .await
            .map_err(tool_error)?
            == "child";
        let items = queries::continue_reading(
            &self.state.db,
            principal.user.id,
            input.limit.unwrap_or(12),
            child,
        )
        .await
        .map_err(tool_error)?;
        Ok(Json(ContinueOutput {
            items: items
                .into_iter()
                .map(|item| ContinueItem {
                    book: library_book(item.book),
                    percentage: item.percentage,
                    updated_at: item.updated_at,
                })
                .collect(),
        }))
    }

    #[tool(
        description = "Get this book for the signed-in profile. Adults reuse an owned copy or start an acquisition; children create a request for an adult to approve."
    )]
    async fn add_book(
        &self,
        Parameters(input): Parameters<AddBookInput>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<Json<IntentOutput>, McpError> {
        let principal = principal(&ctx)?;
        require_write(&principal)?;
        let outcome = services::books::add(
            &self.state,
            &principal.user,
            input.book_id,
            input.send_to_reader.unwrap_or(false),
        )
        .await
        .map_err(tool_error)?;
        Ok(Json(IntentOutput {
            book_id: outcome.book_id,
            phase: outcome.phase,
            message: outcome.message,
            request_id: outcome.request_id,
            acquisition_id: outcome.acquisition_id,
        }))
    }

    #[tool(
        description = "Acquire a catalogue result for this adult profile using provider + providerKey from search_books. Children should use request_book instead. Returns acquisition status and whether active work was reused."
    )]
    async fn add_catalogue_book(
        &self,
        Parameters(input): Parameters<AddCatalogueBookInput>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<Json<CatalogueAcquisitionOutput>, McpError> {
        let principal = principal(&ctx)?;
        require_write(&principal)?;
        let outcome = services::books::add_catalogue(
            &self.state,
            &principal.user,
            &input.provider,
            &input.provider_key,
            input.preferred_format,
            input.preferred_language,
            input.send_to_reader.unwrap_or(false),
        )
        .await
        .map_err(tool_error)?;
        Ok(Json(CatalogueAcquisitionOutput {
            id: outcome.id,
            status: outcome.status.as_str().to_string(),
            duplicate: outcome.duplicate,
            book_id: outcome.book_id,
        }))
    }

    #[tool(
        description = "Send an owned book to the profile's reader (default target unless targetId is given)."
    )]
    async fn send_to_reader(
        &self,
        Parameters(input): Parameters<SendToReaderInput>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<Json<DeliveryOutput>, McpError> {
        let principal = principal(&ctx)?;
        require_write(&principal)?;
        let delivery = services::delivery::send_book(
            &self.state,
            &principal.user,
            input.book_id,
            input.target_id,
        )
        .await
        .map_err(tool_error)?;
        Ok(Json(DeliveryOutput {
            book_id: delivery.book_id,
            file_id: delivery.file_id,
            status: delivery.status,
            address: delivery.address,
            error_message: delivery.error_message,
        }))
    }

    #[tool(
        description = "List book requests: the whole household for adults, the profile's own for children."
    )]
    async fn list_requests(
        &self,
        ctx: RequestContext<RoleServer>,
    ) -> Result<Json<RequestsOutput>, McpError> {
        let principal = principal(&ctx)?;
        let items = services::requests::list(&self.state, &principal.user)
            .await
            .map_err(tool_error)?;
        Ok(Json(RequestsOutput {
            items: items.into_iter().map(request_output).collect(),
        }))
    }

    #[tool(
        description = "Ask an adult for a book by its catalogue identity (provider + providerKey from search_books)."
    )]
    async fn request_book(
        &self,
        Parameters(input): Parameters<RequestBookInput>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<Json<IntentOutput>, McpError> {
        let principal = principal(&ctx)?;
        require_write(&principal)?;
        let outcome = services::requests::create(
            &self.state,
            &principal.user,
            &input.provider,
            &input.provider_key,
        )
        .await
        .map_err(tool_error)?;
        Ok(Json(IntentOutput {
            book_id: outcome.request.book_id,
            phase: outcome.request.phase.clone(),
            message: if outcome.duplicate {
                "This book is already requested.".to_string()
            } else {
                "Requested; an adult in your household will decide.".to_string()
            },
            request_id: Some(outcome.request.id),
            acquisition_id: outcome.request.acquisition_id,
        }))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for BokhylleMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_instructions(
                "Bokhylle household book service. Search the library or the public catalogue, \
                 inspect books and reading progress, add owned or catalogue books (adults acquire, children ask an \
                 adult), and send owned books to the profile's reader. One token is one Bokhylle \
                 profile; its permissions always apply."
                    .to_string(),
            )
    }
}
