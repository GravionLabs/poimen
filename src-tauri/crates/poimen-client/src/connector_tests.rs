//! Requests, subscriptions and attachments over a [`pipe::Connector`] (the ssh transport).

use super::*;

/// A connector whose every connection is served by `serve` on a thread, over OS pipes.
struct FakeConnector {
    serve: fn(BufReader<io::PipeReader>, io::PipeWriter),
}

impl pipe::Connector for FakeConnector {
    fn connect(&self) -> io::Result<pipe::Pipe> {
        let (to_server_r, to_server_w) = io::pipe()?;
        let (from_server_r, from_server_w) = io::pipe()?;
        let serve = self.serve;
        thread::spawn(move || serve(BufReader::new(to_server_r), from_server_w));
        Ok(pipe::Pipe::new(from_server_r, to_server_w, || {}))
    }
}

fn answer(reader: &mut BufReader<io::PipeReader>, out: &mut io::PipeWriter, result: &str) {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let id = serde_json::from_str::<Value>(&line).unwrap()["id"].clone();
    writeln!(out, r#"{{"id":{id},"result":{result}}}"#).unwrap();
}

#[test]
fn a_connector_client_sends_requests_over_its_pipe() {
    let client = Client::with_connector(
        "ssh:box",
        Arc::new(FakeConnector {
            serve: |mut r, mut w| {
                answer(
                    &mut r,
                    &mut w,
                    r#"{"spaces":[{"id":"s1","name":"a","cwd":"/x"}]}"#,
                )
            },
        }),
    );
    assert_eq!(client.list_spaces().unwrap()[0].name, "a");
    assert_eq!(client.socket(), Path::new("ssh:box"));
}

#[test]
fn ping_and_subscribe_shares_one_connection_and_ends_with_it() {
    let client = Client::with_connector(
        "ssh:box",
        Arc::new(FakeConnector {
            serve: |mut r, mut w| {
                let ping = format!(
                    r#"{{"version":"0.9.0","protocol":{},"pid":7}}"#,
                    protocol::PROTOCOL_VERSION
                );
                answer(&mut r, &mut w, &ping);
                answer(&mut r, &mut w, "{}");
                writeln!(
                    w,
                    r#"{{"event":"terminal.closed","data":{{"terminal_id":"t1","space_id":"s1"}}}}"#
                )
                .unwrap();
            },
        }),
    );
    let (ping, stream) = client.ping_and_subscribe(None).unwrap();
    assert_eq!(ping.version, "0.9.0");
    let mut stream = stream.expect("a stream for the same protocol");
    assert!(stream.next().unwrap().is_ok());
    assert!(stream.next().unwrap().is_err(), "the end of the connection");
}

#[test]
fn ping_and_subscribe_does_not_subscribe_to_another_protocol() {
    let client = Client::with_connector(
        "ssh:box",
        Arc::new(FakeConnector {
            serve: |mut r, mut w| {
                answer(
                    &mut r,
                    &mut w,
                    r#"{"version":"0.1.0","protocol":1,"pid":7}"#,
                );
                // A subscribe would be a second line; none may come.
                let mut rest = String::new();
                assert_eq!(r.read_line(&mut rest).unwrap(), 0, "{rest}");
            },
        }),
    );
    let (ping, stream) = client.ping_and_subscribe(None).unwrap();
    assert_eq!((ping.protocol, stream.is_none()), (1, true));
}

#[test]
fn a_failing_connector_is_a_connect_error() {
    struct Down;
    impl pipe::Connector for Down {
        fn connect(&self) -> io::Result<pipe::Pipe> {
            Err(io::Error::other("offline"))
        }
    }
    let err = Client::with_connector("ssh:box", Arc::new(Down))
        .ping()
        .unwrap_err();
    assert!(err.is_not_running(), "{err}");
}
