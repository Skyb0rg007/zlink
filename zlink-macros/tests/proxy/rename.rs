#[tokio::test]
async fn rename_test() {
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use zlink::{Connection, proxy, test_utils::mock_socket::MockSocket};

    #[proxy("org.example.Rename")]
    trait RenameProxy {
        #[zlink(rename = "GetData")]
        async fn get_data(&mut self) -> zlink::Result<Result<GetDataReply<'_>, Error>>;

        #[zlink(rename = "SetValue")]
        async fn update_value(&mut self, value: i32) -> zlink::Result<Result<(), Error>>;

        // Test snake_case to PascalCase conversion
        async fn snake_case_method(&mut self) -> zlink::Result<Result<(), Error>>;
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct Error;

    #[derive(Debug, Serialize, Deserialize)]
    struct GetDataReply<'a> {
        #[serde(borrow)]
        data: &'a str,
    }

    // Test get_data with renamed method
    let responses = json!({"parameters": {"data": "test data"}}).to_string();
    let socket = MockSocket::with_responses(&[&responses]);
    let mut conn = Connection::new(socket);

    let result = conn.get_data().await.unwrap().unwrap();
    assert_eq!(result.data, "test data");

    // Verify the renamed method name is used
    let bytes_written = conn.write().write_half().written_data();
    let written: serde_json::Value =
        serde_json::from_slice(&bytes_written[..bytes_written.len() - 1]).unwrap();
    assert_eq!(written["method"], "org.example.Rename.GetData");

    // Test update_value
    let responses = json!({}).to_string();
    let socket = MockSocket::with_responses(&[&responses]);
    let mut conn = Connection::new(socket);

    conn.update_value(42).await.unwrap().unwrap();

    // Test snake_case_method (should be converted to PascalCase)
    let responses = json!({}).to_string();
    let socket = MockSocket::with_responses(&[&responses]);
    let mut conn = Connection::new(socket);

    conn.snake_case_method().await.unwrap().unwrap();

    // Verify snake_case was converted to PascalCase
    let bytes_written = conn.write().write_half().written_data();
    let written: serde_json::Value =
        serde_json::from_slice(&bytes_written[..bytes_written.len() - 1]).unwrap();
    assert_eq!(written["method"], "org.example.Rename.SnakeCaseMethod");
}

#[tokio::test]
async fn param_rename_chain_test() {
    use futures_util::{pin_mut, stream::StreamExt};
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use zlink::{Connection, proxy, test_utils::mock_socket::MockSocket};

    #[derive(Debug, Serialize, Deserialize)]
    struct Error;

    #[proxy("org.example.ParamRename")]
    trait ParamRenameProxy {
        #[allow(unused)]
        async fn set_config(
            &mut self,
            #[zlink(rename = "dryRun")] dry_run: bool,
            #[zlink(rename = "configValue")] config_value: String,
        ) -> zlink::Result<Result<(), Error>>;
    }

    // Test chain_* method with renamed parameters
    let reply1 = json!({}).to_string();
    let reply2 = json!({}).to_string();
    let socket = MockSocket::new(&[&reply1, &reply2], vec![vec![]]);
    let mut conn = Connection::new(socket);

    {
        let replies = conn
            .chain_set_config(true, "test_value".to_string())
            .unwrap()
            .set_config(false, "another_value".to_string())
            .unwrap()
            .send::<(), Error>()
            .await
            .unwrap();

        pin_mut!(replies);

        let (reply1, _fds) = replies.next().await.unwrap().unwrap();
        reply1.unwrap();
        let (reply2, _fds) = replies.next().await.unwrap().unwrap();
        reply2.unwrap();
    }

    // Verify both chain_* and chain extension methods use renamed parameters
    let bytes_written = conn.write().write_half().written_data();

    // Parse the two JSON messages (separated by null bytes)
    let messages: Vec<&[u8]> = bytes_written
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .collect();
    assert_eq!(messages.len(), 2);

    // Check first message (from chain_set_config)
    let written1: serde_json::Value = serde_json::from_slice(messages[0]).unwrap();
    assert_eq!(written1["method"], "org.example.ParamRename.SetConfig");
    assert_eq!(written1["parameters"]["dryRun"], true);
    assert_eq!(written1["parameters"]["configValue"], "test_value");

    // Check second message (from chain extension .set_config)
    let written2: serde_json::Value = serde_json::from_slice(messages[1]).unwrap();
    assert_eq!(written2["method"], "org.example.ParamRename.SetConfig");
    assert_eq!(written2["parameters"]["dryRun"], false);
    assert_eq!(written2["parameters"]["configValue"], "another_value");
}
