## How to test docs.rs changes

Set up your local docs.rs environment as per official README:  
https://github.com/rust-lang/docs.rs?tab=readme-ov-file#getting-started

Make sure you have:
- Your .env contents exported
- docker-compose for db and s3 running
- The web server running via local (or pure docker-compose approach)
- If on a remote machine, port 3000 (or whatever your webserver is listening on) forwarded

Invoke the build command against your local path:

```
# you could also point to the `cratesfyi` binary from outside of the docker workspace,
# though you'll still need the right ENVs exported
cargo run -- build crate --local ../tokio-metrics
```

View the generated documentation for `tokio-metrics` in your browser