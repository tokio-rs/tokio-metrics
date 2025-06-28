#[tokio::main]
async fn main() {
    // construct a TaskMonitor for each endpoint
    let monitor_root = tokio_metrics::TaskMonitor::new();

    let monitor_create_user = CreateUserMonitors {
        // monitor for the entire endpoint
        route: tokio_metrics::TaskMonitor::new(),
        // monitor for database insertion subtask
        insert: tokio_metrics::TaskMonitor::new(),
    };

    // build our application with two instrumented endpoints
    let app = axum::Router::new()
        // `GET /` goes to `root`
        .route(
            "/",
            axum::routing::get({
                let monitor = monitor_root.clone();
                move || monitor.instrument(async { "Hello, World!" })
            }),
        )
        // `POST /users` goes to `create_user`
        .route(
            "/users",
            axum::routing::post({
                let monitors = monitor_create_user.clone();
                let route = monitors.route.clone();
                move |payload| route.instrument(create_user(payload, monitors))
            }),
        );

    // print task metrics for each endpoint every 1s
    let metrics_frequency = std::time::Duration::from_secs(1);
    tokio::spawn(async move {
        let root_intervals = monitor_root.intervals();
        let create_user_route_intervals = monitor_create_user.route.intervals();
        let create_user_insert_intervals = monitor_create_user.insert.intervals();
        let create_user_intervals = create_user_route_intervals.zip(create_user_insert_intervals);

        let intervals = root_intervals.zip(create_user_intervals);
        for (root_route, (create_user_route, create_user_insert)) in intervals {
            println!("root_route = {root_route:#?}");
            println!("create_user_route = {create_user_route:#?}");
            println!("create_user_insert = {create_user_insert:#?}");
            tokio::time::sleep(metrics_frequency).await;
        }
    });

    // run the server
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 3000));
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn create_user(
    axum::Json(payload): axum::Json<CreateUser>,
    monitors: CreateUserMonitors,
) -> impl axum::response::IntoResponse {
    let user = User {
        id: 1337,
        username: payload.username,
    };
    // instrument inserting the user into the db:
    monitors.insert.instrument(insert_user(user.clone())).await;
    (axum::http::StatusCode::CREATED, axum::Json(user))
}

#[derive(Clone)]
struct CreateUserMonitors {
    // monitor for the entire endpoint
    route: tokio_metrics::TaskMonitor,
    // monitor for database insertion subtask
    insert: tokio_metrics::TaskMonitor,
}

#[derive(serde::Deserialize)]
struct CreateUser {
    username: String,
}
#[derive(Clone, serde::Serialize)]
struct User {
    id: u64,
    username: String,
}

// insert the user into the database
async fn insert_user(_: User) {
    /* talk to database */
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
}
