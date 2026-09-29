use reqwest::Response;

#[derive(Debug, thiserror::Error)]
pub(crate) enum BodyReadError {
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    #[error("response exceeds the size limit")]
    TooLarge,
}

/// Enforce the cap while reading as well as against Content-Length: download
/// clients and indexers may omit that header or send a chunked response.
pub(crate) async fn read_limited(
    mut response: Response,
    max_bytes: u64,
) -> Result<Vec<u8>, BodyReadError> {
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes)
    {
        return Err(BodyReadError::TooLarge);
    }

    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if chunk.len() as u64 > max_bytes.saturating_sub(bytes.len() as u64) {
            return Err(BodyReadError::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn rejects_chunked_responses_without_content_length() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n9\r\n123456789\r\n0\r\n\r\n")
                .await
                .unwrap();
        });

        let response = reqwest::get(format!("http://{address}")).await.unwrap();
        assert!(response.content_length().is_none());
        assert!(matches!(
            read_limited(response, 8).await,
            Err(BodyReadError::TooLarge)
        ));
        server.await.unwrap();
    }
}
