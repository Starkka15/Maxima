use anyhow::Result;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpListener;

pub async fn execute() -> Result<Option<String>> {
    // Hardcoded for now, need to figure out where pc_sign comes from
    open::that("https://accounts.ea.com/connect/auth?response_type=token&client_id=JUNO_PC_CLIENT&pc_sign=eyJhdiI6InYxIiwiYnNuIjoiRGVmYXVsdCBzdHJpbmciLCJnaWQiOjc5NDQsImhzbiI6IkFBMDAwMDAwMDAwMDAwMDAxMjc3IiwibWFjIjoiJGI0MmU5OTRjNTBhZiIsIm1pZCI6IjUyODUwNDMyMDkxOTEyODgwNDMiLCJtc24iOiJEZWZhdWx0IHN0cmluZyIsInN2IjoidjIiLCJ0cyI6IjIwMjMtMi0xMiAxMzo0NTozNjo5MzcifQ.c__XyfI01HjScx1yJ4JpZWklwMO9qn4iC9OQ5oJFE3A")?;
    let listener = TcpListener::bind("127.0.0.1:31033").await?;

    loop {
        let (mut socket, _) = listener.accept().await?;

        let (read, _) = socket.split();
        let mut reader = BufReader::new(read);

        let mut line = String::new();
        reader.read_line(&mut line).await?;

        if line.starts_with("GET /auth") {
            let query_string = line
                .split_once("?")
                .map(|(_, qs)| qs.trim())
                .map(querystring::querify)
                .unwrap();

            for query in query_string {
                if query.0 == "access_token" {
                    return Ok(Some(query.1.to_string()));
                }
            }

            return Ok(None);
        }
    }
}
