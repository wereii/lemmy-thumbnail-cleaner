use isahc::http::Uri;
use isahc::{Body, HttpClient, ReadResponseExt};
use std::time::Duration;
use std::{env, iter, thread};

use log::{debug, error, info, warn};
use postgres::fallible_iterator::FallibleIterator;
use postgres::{Client, NoTls};
use url::Url;

use env_logger;

const DEFAULT_DATABASE_URI: &str = "postgres://lemmy@localhost:5432/lemmy";
const DEFAULT_PICTRS_HOST: &str = "pictrs:8080";
const DEFAULT_THUMBNAIL_MIN_AGE_MONTHS: u64 = 3;
const DEFAULT_QUERY_LIMIT: u64 = 300;

const CHECK_INTERVAL: Duration = Duration::from_secs(300);

// "Runtime" formatting :tm:
macro_rules! base_thumbnail_query_fmt {
    () => {
        "SELECT {select_target} FROM post \
        WHERE thumbnail_url IS NOT NULL \
        AND published < now() + interval '{interval} months' \
        AND thumbnail_url LIKE '{base_host}%' \
        {query_suffix};"
    };
}

fn main() {
    env_logger::init();
    info!("Starting thumbnail cleaner");

    // TODO: Extract local site host from database:
    // `select actor_id from site join local_site on local_site.id = site.id where local_site.id = 1;`
    let instance_host_url = {
        let value = env::var("INSTANCE_HOST").unwrap_or_else(|err| {
            error!("INSTANCE_HOST is required, exiting! (err: {})", err);
            std::process::exit(1);
        });

        let url = Url::parse(value.as_str()).unwrap_or_else(|err| {
            error!("Error parsing INSTANCE_HOST URL '{}', exiting!", err);
            std::process::exit(1);
        });

        if url.scheme() != "https" {
            warn!("INSTANCE_HOST does not have HTTPS scheme, are you sure this is correct?");
        }
        url
    };

    let check_interval: Duration = match env::var("CHECK_INTERVAL") {
        Ok(val) => {
            let val: u64 = val.parse().unwrap_or_else(|err| {
                error!("Error parsing CHECK_INTERVAL '{}', exiting!", err);
                std::process::exit(1)
            });

            info!("CHECK_INTERVAL set to '{}' seconds", val);
            Duration::from_secs(val)
        }
        Err(_) => {
            info!(
                "CHECK_INTERVAL not set, using default {} seconds",
                CHECK_INTERVAL.as_secs()
            );
            CHECK_INTERVAL
        }
    };

    let query_limit: u64 = match env::var("QUERY_LIMIT") {
        Ok(val) => {
            let parsed: u64 = val.parse().unwrap_or_else(|err| {
                error!("Error parsing QUERY_LIMIT '{}', exiting!", err);
                std::process::exit(1)
            });

            info!("QUERY_LIMIT set to '{}'", parsed);
            parsed
        }
        Err(_) => {
            info!(
                "QUERY_LIMIT not set, using default: {}",
                DEFAULT_QUERY_LIMIT
            );
            DEFAULT_QUERY_LIMIT
        }
    };

    let thumbnail_min_age_months: u64 = match env::var("THUMBNAIL_MIN_AGE_MONTHS") {
        Ok(val) => {
            let val: u64 = val.parse().unwrap_or_else(|err| {
                error!("Error parsing THUMBNAIL_MIN_AGE_MONTHS '{}', exiting!", err);
                std::process::exit(1)
            });

            info!("THUMBNAIL_MIN_AGE_MONTHS set to '{}'", val);
            val
        }
        Err(_) => {
            info!(
                "THUMBNAIL_MIN_AGE_MONTHS not set, using default: {}",
                DEFAULT_THUMBNAIL_MIN_AGE_MONTHS
            );
            DEFAULT_THUMBNAIL_MIN_AGE_MONTHS
        }
    };

    let mut pg_client = {
        let database_uri_env = env::var("DATABASE_URI").unwrap_or_else(|_| {
            warn!(
                "DATABASE_URI not set, using default: '{}'",
                DEFAULT_DATABASE_URI
            );
            DEFAULT_DATABASE_URI.to_string()
        });

        Client::connect(database_uri_env.as_str(), NoTls).unwrap_or_else(|err| {
            error!("Failed to connect to database: {}", err);
            std::process::exit(1);
        })
    };

    let pictrs_host = {
        env::var("PICTRS_HOST").unwrap_or_else(|_| {
            warn!(
                "PICTRS_HOST not set, using default: '{}'",
                DEFAULT_PICTRS_HOST
            );
            DEFAULT_PICTRS_HOST.to_string()
        })
    };

    let http_client = {
        let pictrs_api_key = env::var("PICTRS_API_KEY").expect("PICTRS_API_KEY not set");
        HttpClient::builder()
            .default_header("x-api-token", pictrs_api_key.as_str())
            .build()
            .expect("Failed to create HTTP client")
    };

    let count_query = format!(
        base_thumbnail_query_fmt!(),
        select_target = "COUNT(*)",
        interval = thumbnail_min_age_months,
        base_host = instance_host_url.as_str(),
        query_suffix = "",
    );

    let thumbnail_query = format!(
        base_thumbnail_query_fmt!(),
        select_target = "thumbnail_url",
        interval = thumbnail_min_age_months,
        base_host = instance_host_url.as_str(),
        query_suffix = "LIMIT ".to_owned() + query_limit.to_string().as_str() + " ;",
    );

    loop {
        info!("Checking for thumbnails to clean");

        let count = {
            let count_rows = pg_client
                .query(count_query.as_str(), &[])
                .expect("Failed to query database for count");

            let count: i64 = count_rows.get(0).expect("No rows returned").get(0);
            info!(
                "Database contains {} of thumbnails that can be cleaned up",
                count
            );
            count
        };

        if count > 0 {
            let thumbnail_urls_rows = pg_client
                .query(thumbnail_query.as_str(), &[])
                .expect("Failed to query database for thumbnails");

            let mut processed = 0;
            for row in thumbnail_urls_rows {
                let thumbnail_url = Url::parse(row.get::<usize, String>(0).as_str())
                    .expect("Failed to parse thumbnail URL");

                let thumbnail_alias = thumbnail_url.path().split("/").last().unwrap();
                // TODO: This isn't really durable check, maybe test for valid `uuid.(png|jpg|webp)`?
                if thumbnail_alias.len() < 36 {
                    warn!(
                        "Thumbnail name '{}' does not look valid, skipping!",
                        thumbnail_alias
                    );
                    continue;
                }

                let mut response = http_client
                    .post(
                        Uri::builder()
                            .scheme("http")
                            .authority(pictrs_host.to_owned())
                            .path_and_query("/internal/delete?alias=".to_owned() + thumbnail_alias)
                            .build()
                            .expect("Failed to build pictrs URL"),
                        Body::empty(),
                    )
                    .expect("pictrs request failed");

                if response.status() == 200 {
                    debug!("pict-rs: thumbnail '{}' deleted", thumbnail_alias);
                } else if response.status() == 404 {
                    warn!("pict-rs: thumbnail '{}' not found?", thumbnail_alias);
                } else {
                    error!(
                        "pict-rs: failed to delete thumbnail '{}'; {} - {:?}",
                        thumbnail_alias,
                        response.status(),
                        response.text(),
                    );
                    continue;
                }

                let mut result_rows_iter = pg_client
                    .query_raw(
                        &("update post set thumbnail_url = null WHERE thumbnail_url = '"
                            .to_string()
                            + thumbnail_url.as_str()
                            + "';"),
                        iter::empty::<Option<i64>>(),
                    )
                    .expect("Database error updating thumbnail");

                // this is awful, but I don't know how to exhaust the iterator without moving it into .for_each or .collect
                while let Some(_) = result_rows_iter
                    .next()
                    .expect("Failed to iterate over results")
                {}

                if result_rows_iter.rows_affected().is_none() {
                    warn!(
                        "postgres returned no rows affected, failed to null thumbnail '{}' ?",
                        thumbnail_alias
                    );
                } else {
                    debug!("postgres: thumbnail '{}' updated to null", thumbnail_alias);
                }

                processed += 1;
                if processed % 10 == 0 {
                    info!("Processed {} thumbnails", processed);
                }
            }
            info!("Finished iteration, processed {} thumbnails", processed);
        }
        info!("Sleeping for {}s", check_interval.as_secs());
        thread::sleep(check_interval);
    }
}
