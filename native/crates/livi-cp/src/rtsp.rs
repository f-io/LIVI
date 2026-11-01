// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub protocol: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct Response {
    pub protocol: Option<String>,
    pub status: Option<u16>,
    pub status_text: Option<String>,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

const HEADER_END: &[u8] = b"\r\n\r\n";

pub fn parse(buf: &[u8]) -> (Vec<Request>, Vec<u8>) {
    let mut out = Vec::new();
    let mut offset = 0;
    while offset < buf.len() {
        let Some(head_len) = find(&buf[offset..], HEADER_END) else {
            break;
        };
        let head_end = offset + head_len;
        let head: String = buf[offset..head_end]
            .iter()
            .map(|&b| char::from(b & 0x7f))
            .collect();
        let mut lines = head.split("\r\n");
        let mut request_line = lines.next().unwrap_or("").split(' ');
        let method = request_line.next().unwrap_or("").to_string();
        let path = request_line.next().unwrap_or("").to_string();
        let protocol = request_line.next().unwrap_or("RTSP/1.0").to_string();

        let mut headers = HashMap::new();
        for line in lines {
            if let Some((name, value)) = line.split_once(':') {
                headers.insert(name.trim().to_lowercase(), value.trim().to_string());
            }
        }

        let body_start = head_end + HEADER_END.len();
        let body_end = body_start + content_length(headers.get("content-length"));
        if body_end > buf.len() {
            break;
        }
        out.push(Request {
            method,
            path,
            protocol,
            headers,
            body: buf[body_start..body_end].to_vec(),
        });
        offset = body_end;
    }
    (out, buf[offset..].to_vec())
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn content_length(value: Option<&String>) -> usize {
    let digits: String = value
        .map(|v| v.chars().take_while(char::is_ascii_digit).collect())
        .unwrap_or_default();
    digits.parse().unwrap_or(0)
}

fn status_text(status: u16) -> &'static str {
    match status {
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    }
}

pub fn build_response(req: &Request, res: Response) -> Vec<u8> {
    let protocol = res.protocol.unwrap_or_else(|| req.protocol.clone());
    let status = res.status.unwrap_or(200);
    let text = res
        .status_text
        .unwrap_or_else(|| status_text(status).to_string());

    let mut headers = res.headers;
    let mut set = |name: &str, value: String| match headers.iter_mut().find(|(k, _)| k == name) {
        Some(slot) => slot.1 = value,
        None => headers.push((name.to_string(), value)),
    };
    if let Some(cseq) = req.headers.get("cseq") {
        set("CSeq", cseq.clone());
    }
    set("Content-Length", res.body.len().to_string());

    let mut head = format!("{protocol} {status} {text}\r\n");
    for (name, value) in &headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    let mut out = head.into_bytes();
    out.extend_from_slice(&res.body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_requests_and_keeps_the_partial_rest() {
        let buf = b"POST /pair-verify RTSP/1.0\r\nCSeq: 3\r\nContent-Length: 4\r\n\r\nbody\
                    GET /info RTSP/1.0\r\nCSeq: 4\r\n\r\n\
                    POST /feedback RTSP/1.0\r\nContent-Length: 10\r\n\r\npart";
        let (reqs, rest) = parse(buf);
        assert_eq!(reqs.len(), 2);
        assert_eq!(reqs[0].method, "POST");
        assert_eq!(reqs[0].path, "/pair-verify");
        assert_eq!(reqs[0].headers["cseq"], "3");
        assert_eq!(reqs[0].body, b"body");
        assert_eq!(reqs[1].path, "/info");
        assert!(reqs[1].body.is_empty());
        assert!(rest.starts_with(b"POST /feedback"));
    }

    #[test]
    fn a_missing_protocol_defaults_to_rtsp() {
        let (reqs, _) = parse(b"OPTIONS *\r\n\r\n");
        assert_eq!(reqs[0].protocol, "RTSP/1.0");
        let (reqs, _) = parse(b"OPTIONS\r\n\r\n");
        assert_eq!(reqs[0].path, "");
    }

    #[test]
    fn content_length_reads_leading_digits() {
        assert_eq!(content_length(Some(&"12abc".to_string())), 12);
        assert_eq!(content_length(Some(&"x".to_string())), 0);
        assert_eq!(content_length(None), 0);
    }

    #[test]
    fn response_echoes_cseq_and_counts_the_body() {
        let (reqs, _) = parse(b"GET /info RTSP/1.0\r\nCSeq: 9\r\n\r\n");
        let res = Response {
            headers: vec![(
                "Content-Type".into(),
                "application/x-apple-binary-plist".into(),
            )],
            body: b"xy".to_vec(),
            ..Default::default()
        };
        assert_eq!(
            build_response(&reqs[0], res),
            b"RTSP/1.0 200 OK\r\nContent-Type: application/x-apple-binary-plist\r\nCSeq: 9\r\nContent-Length: 2\r\n\r\nxy"
        );
        let missing = Response {
            status: Some(404),
            ..Default::default()
        };
        assert_eq!(
            build_response(&reqs[0], missing),
            b"RTSP/1.0 404 Not Found\r\nCSeq: 9\r\nContent-Length: 0\r\n\r\n"
        );
    }
}
