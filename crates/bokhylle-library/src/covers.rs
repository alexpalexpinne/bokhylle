/// Covers below this size are stubs (for example 1x1 transparent PNGs shipped
/// by some releases) and are treated as missing so the placeholder renders.
pub const MIN_COVER_BYTES: usize = 1024;

pub fn usable_cover(bytes: &[u8]) -> bool {
    bytes.len() >= MIN_COVER_BYTES
}

/// Designed fallback covers: restrained Bokhylle fields with the title as the
/// hero, a catalogue code derived from the title, and impressions of the
/// print rules. Deterministic for a given title so a book always looks the
/// same, and deliberately not a gradient.
pub fn placeholder_svg(title: &str, authors: &[String]) -> Vec<u8> {
    const PALETTE: [(&str, &str); 6] = [
        ("#1a1511", "#f1e9da"),
        ("#a3461f", "#fdf7ef"),
        ("#56624a", "#f1e9da"),
        ("#692f38", "#f1e9da"),
        ("#22303c", "#f1e9da"),
        ("#584a35", "#f1e9da"),
    ];

    let hash = hash_for(title);
    let (field, ink) = PALETTE[(hash % PALETTE.len() as u32) as usize];
    let code = format!("B{:04}", hash % 10_000);
    let lines = wrap_title(title, 12, 3);
    let size = match lines.len() {
        1 => 62,
        2 => 50,
        _ => 40,
    };
    let line_height = (f64::from(size) * 1.2) as u32;
    let block_top = 300 - (line_height * (lines.len() as u32 - 1)) / 2;
    let title_lines: String = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let line_size = (292.0 / estimated_em_width(line))
                .floor()
                .min(f64::from(size)) as u32;
            format!(
                r#"<text x="200" y="{}" text-anchor="middle" font-family="Georgia, 'Times New Roman', serif" font-size="{line_size}" fill="{ink}">{}</text>"#,
                block_top + index as u32 * line_height,
                escape_xml(line)
            )
        })
        .collect::<Vec<_>>()
        .join("\n  ");

    let author = escape_xml(&truncate(&authors.join(", "), 40));
    let author = if author.is_empty() {
        "Unknown author".to_string()
    } else {
        author
    };

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 600">
  <rect width="400" height="600" fill="{field}"/>
  <rect x="18" y="18" width="364" height="564" fill="none" stroke="{ink}" stroke-opacity="0.35"/>
  <text x="40" y="52" font-family="Helvetica, Arial, sans-serif" font-size="14" letter-spacing="6" fill="{ink}" fill-opacity="0.85">BOKHYLLE</text>
  <line x1="40" y1="68" x2="360" y2="68" stroke="{ink}" stroke-opacity="0.35"/>
  {title_lines}
  <text x="200" y="470" text-anchor="middle" font-family="Helvetica, Arial, sans-serif" font-size="20" letter-spacing="2" fill="{ink}" fill-opacity="0.9">{author}</text>
  <line x1="40" y1="520" x2="360" y2="520" stroke="{ink}" stroke-opacity="0.35"/>
  <text x="40" y="552" font-family="Helvetica, Arial, sans-serif" font-size="12" letter-spacing="2" fill="{ink}" fill-opacity="0.75">PRIVATE LIBRARY</text>
  <text x="360" y="552" text-anchor="end" font-family="Helvetica, Arial, sans-serif" font-size="12" letter-spacing="2" fill="{ink}" fill-opacity="0.75">{code}</text>
</svg>
"##
    )
    .into_bytes()
}

/// Conservative text width in ems. Cover titles use Georgia but the SVG can
/// render with a fallback font, so leave room inside the 320px text area.
fn estimated_em_width(line: &str) -> f64 {
    line.chars()
        .map(|character| match character {
            ' ' => 0.28,
            'W' | 'M' | 'm' | 'w' => 0.9,
            'i' | 'l' | 'I' | '!' | '.' | ',' | ':' | ';' => 0.3,
            'f' | 'j' | 'r' | 't' => 0.42,
            'A'..='Z' => 0.72,
            'a'..='z' => 0.56,
            _ => 0.8,
        })
        .sum::<f64>()
        .max(1.0)
}

/// Greedy word wrap for the cover title; long words are hard-split and the
/// last line gets an ellipsis when the title does not fit.
fn wrap_title(title: &str, max_chars: usize, max_lines: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();

    for word in title.split_whitespace() {
        let word = word.to_string();
        loop {
            let candidate_len = if current.is_empty() {
                word.chars().count()
            } else {
                current.chars().count() + 1 + word.chars().count()
            };

            if candidate_len <= max_chars {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(&word);
                break;
            }

            if current.is_empty() {
                // Hard-split a word that alone exceeds the line.
                let mut chunk = String::new();
                for character in word.chars() {
                    if chunk.chars().count() == max_chars {
                        lines.push(std::mem::take(&mut chunk));
                    }
                    chunk.push(character);
                }
                current = chunk;
                break;
            }

            lines.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }

    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            let trimmed: String = last.chars().take(max_chars.saturating_sub(1)).collect();
            *last = format!("{}…", trimmed.trim_end());
        }
    }

    if lines.is_empty() {
        lines.push("Untitled".to_string());
    }

    lines
}

fn hash_for(title: &str) -> u32 {
    let mut hash: u32 = 2_166_136_261;
    for byte in title.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    hash
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_is_a_designed_bokhylle_edition() {
        let svg = String::from_utf8(placeholder_svg(
            "Project <Hail> Mary",
            &["Andy Weir".into()],
        ))
        .unwrap();
        assert!(svg.contains("BOKHYLLE"));
        assert!(svg.contains("PRIVATE LIBRARY"));
        assert!(svg.contains("Project"));
        assert!(svg.contains("&lt;Hail&gt;"));
        assert!(svg.contains("Andy Weir"));
        assert!(
            !svg.contains("linearGradient"),
            "no gradients in a Bokhylle edition"
        );
        assert!(svg.contains("B<span") || svg.contains(">B"));
    }

    #[test]
    fn stub_covers_are_not_usable() {
        assert!(!usable_cover(&[]));
        assert!(!usable_cover(&[0x89, b'P', b'N', b'G']));
        assert!(!usable_cover(&vec![0u8; MIN_COVER_BYTES - 1]));
        assert!(usable_cover(&vec![0u8; MIN_COVER_BYTES]));
    }

    #[test]
    fn long_titles_wrap_and_ellipsize() {
        let svg = String::from_utf8(placeholder_svg(
            "A Rather Long Book Title That Exceeds The Limit By Quite A Lot",
            &[],
        ))
        .unwrap();
        assert!(svg.contains('…'));
        assert!(svg.contains("Unknown author"));
    }

    #[test]
    fn wraps_and_hard_splits_long_words() {
        assert_eq!(wrap_title("Dune", 16, 3), vec!["Dune"]);
        assert_eq!(
            wrap_title("The Long Way to a Small Angry Planet", 16, 3),
            vec!["The Long Way to", "a Small Angry", "Planet"]
        );
        let split = wrap_title("Supercalifragilisticexpialidocious", 10, 3);
        assert_eq!(split.len(), 3);
        assert!(split[2].ends_with('…'));
    }
}
