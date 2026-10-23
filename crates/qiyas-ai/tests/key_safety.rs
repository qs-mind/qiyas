//! Credential destination controls use synthetic keys and local mock servers only.

mod tests {
    use qiyas_ai::endpoint::normalize;
    #[test]
    fn destinations_are_complete_and_secure() {
        for bad in [
            "http://example.com/v1",
            "https://",
            "https://api",
            "https://api.",
            "https://a..com",
            "https://a.com?key=x",
            "https://u:p@a.com",
            "https://a.com/#x",
            "https://a.com\\evil",
        ] {
            assert!(normalize(bad).is_err(), "{bad}");
        }
        for good in [
            "http://localhost:123/v1",
            "http://127.0.0.1:123/v1",
            "http://[::1]:123/v1",
            "https://example.test/v1",
        ] {
            assert!(normalize(good).is_ok(), "{good}");
        }
        assert_eq!(
            normalize(" https://EXAMPLE.test:443/v1/ ").unwrap(),
            "https://example.test/v1"
        );
        assert_ne!(
            normalize("https://example.test/a"),
            normalize("https://example.test/b")
        );
    }
}

mod network_tests {
    #[tokio::test]
    async fn model_discovery_never_follows_a_redirect_with_credentials() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let first = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let second = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", first.local_addr().unwrap());
        let location = format!("http://{}/models", second.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = first.accept().await.unwrap();
            let mut buf = [0; 4096];
            let mut request = Vec::new();
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let read = socket.read(&mut buf).await.unwrap();
                assert!(read > 0, "Request ended before its headers");
                request.extend_from_slice(&buf[..read]);
            }
            socket.write_all(format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        });
        assert!(qiyas_ai::models::list_models(
            qiyas_ai::models::ModelSource::OpenAiCompatible { base_url: &base },
            Some("mock-only-key")
        )
        .await
        .is_err());
        server.await.unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), second.accept())
                .await
                .is_err()
        );
    }
}
