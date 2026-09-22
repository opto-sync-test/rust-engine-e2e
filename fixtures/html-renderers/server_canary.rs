use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum_leptos_dioxus_unified_ssr::{
    routes,
    server::{config::Config, state::AppState},
};
use sea_orm::DatabaseConnection;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 38_081);
    let state = AppState::new(
        DatabaseConnection::default(),
        Config {
            database_url: "postgres://renderer-canary.invalid/unavailable".to_owned(),
            supabase_url: "http://127.0.0.1:1".to_owned(),
            supabase_anon_key: "not-a-credential".to_owned(),
            supabase_jwt_secret: None,
            bind_addr,
            run_migrations: false,
            secure_cookies: false,
        },
    )?;

    let listener = TcpListener::bind(bind_addr).await?;
    println!("opto-sync renderer canary listening on http://{bind_addr}");
    axum::serve(listener, routes::router(state)).await?;
    Ok(())
}
