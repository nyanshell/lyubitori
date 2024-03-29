use std::env;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::fs::{File, create_dir_all};
use std::{thread, time::Duration};

use clap::Parser;
use tokio::runtime::Runtime;
use futures::{stream, StreamExt};

use egg_mode::raw::auth::{RequestBuilder, Method};
use egg_mode::raw::{ParamList, response_json, response_raw_bytes};
use egg_mode::Response;


static TWEET_FAV_LIST_API: &str = "https://api.twitter.com/1.1/favorites/list.json";
static TWEET_USER_SETTING_API: &str = "https://api.twitter.com/1.1/account/settings.json";
const CONCURRENT_DOWNLOAD_REQUESTS: usize = 50;

#[derive(clap::ValueEnum, Clone)]
enum Operation {
    Download,
}

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Cli {

    #[clap(value_enum)]
    operation: Operation,

    /// set image save path
    #[clap(short, long, value_parser, value_name = "FILE")]
    save_path: Option<PathBuf>,

    /// whether scan all history or fetch recent 200 favs
    #[clap(long, value_parser)]
    scanall: bool,

}

struct TweetsImagesDownloadController {
    client_key: String,
    client_secret: String,
    resource_owner_key: String,
    resource_owner_secret: String,
}

impl TweetsImagesDownloadController {
    fn new(client_key: String,
           client_secret: String,
           resource_owner_key: String,
           resource_owner_secret: String)
           -> TweetsImagesDownloadController {
        TweetsImagesDownloadController {
            client_key,
            client_secret,
            resource_owner_key,
            resource_owner_secret
        }
    }

    fn from_envvar() -> TweetsImagesDownloadController {
        let client_key = env::var("APP_CLIENT_KEY").expect("No APP_CLIENT_KEY in environment variables.");
        let client_secret = env::var("APP_CLIENT_SECRET").expect("No APP_CLIENT_SECRET in environment variables.");
        let resource_owner_key = env::var("RESOURCE_OWNER_KEY").expect("No RESOURCE_OWNER_KEY in environment variables.");
        let resource_owner_secret = env::var("RESOURCE_OWNER_SECRET").expect("No RESOURCE_OWNER_SECRET in environment variables.");
        Self::new(client_key, client_secret, resource_owner_key, resource_owner_secret)
    }

    fn get_tokens(&self) -> Result<(egg_mode::KeyPair, egg_mode::KeyPair), Box<dyn std::error::Error>> {
        let ck = &self.client_key.clone();
        let cs = &self.client_secret.clone();
        let conn_token = egg_mode::KeyPair::new(ck.clone(), cs.clone());

        let rk = &self.resource_owner_key.clone();
        let rs = &self.resource_owner_secret.clone();
        let resource_token = egg_mode::KeyPair::new(rk.clone(), rs.clone());

        Ok((conn_token, resource_token))
    }

    fn get_screen_name(&self) -> Result<String, Box<dyn std::error::Error>> {
        let (conn_token, resource_token) = &self.get_tokens()?;
        let request = RequestBuilder::new(Method::GET, TWEET_USER_SETTING_API)
            .request_keys(conn_token, Some(resource_token));
        let rt = Runtime::new().unwrap();
        let json: Response<serde_json::Value> = rt.block_on(response_json(request))?;
        let screen_name = &json.response["screen_name"].as_str().expect("no screen name").to_string();
        Ok(screen_name.to_string())
    }

    fn update_images(&self, tweets: Vec<serde_json::value::Value>, save_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let mut media_urls = vec![];
        for tweet in tweets {
            if tweet["extended_entities"]["media"].is_array() {
                let media_list: Vec<serde_json::value::Value> = tweet["extended_entities"]["media"].as_array().ok_or("no media")?.to_vec();
                for media in media_list {
                    let media_type = &media["type"].as_str().expect("no media type").to_string();
                    let url = &media["media_url_https"].as_str().expect("no media https url").to_string();
                    match media_type.as_str() {
                        "photo" =>  {
                            media_urls.push(url.clone())
                        },
                        "video" => {
                            media_urls.push(url.clone()); // download thumbnail
                            let video_url = find_max_bitrate_url(&media["video_info"]["variants"].as_array().expect("variants should be an list").to_vec());
                            media_urls.push(video_url?.clone())
                        },
                        "animated_gif" => println!("TODO: download {} gif", media_type),
                        _ => println!("unsupported media type {}", media_type),
                    }
                }

            }
        }
        println!("parsed {} image urls", &media_urls.len());
        // download images
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let (conn_token, resource_token) = &self.get_tokens().unwrap();
            let responses = stream::iter(&media_urls).map(|url| {
                let save_path = &save_path;
                let conn_token = &conn_token;
                let resource_token = &resource_token;
                async move {
                    match save_content(conn_token, resource_token, &url.to_string(), save_path).await {
                        Ok(()) => {
                            println!("{} downloaded", &url);
                            1
                        },
                        Err(e) => {
                            eprintln!("Got an error: {}", e);
                            0
                        }
                    }
                }
            }).buffer_unordered(CONCURRENT_DOWNLOAD_REQUESTS);  // media_urls.len()
            let download_cnt: u8 = responses.collect::<Vec<u8>>().await.iter().sum();
            println!("total downloaded: {}/{}", download_cnt, &media_urls.len());
        });
        Ok(())
    }

    fn download_favorited_images(&self, scanall: bool, save_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let mut prev_id = 0;
        loop {
            let screen_name = &self.get_screen_name().unwrap();
            let (conn_token, resource_token) = &self.get_tokens()?;
            println!("prev id: {}", prev_id);
            let params = match prev_id {
                0 => {
                    ParamList::new()
                        .add_param("screen_name", screen_name.to_string())
                        .add_param("count", 200u8.to_string()) // max return count is 200
                },
                _ => {
                    println!("call with max id: {}", prev_id - 1);
                    ParamList::new()
                        .add_param("screen_name", screen_name.to_string())
                        .add_param("count", 200u8.to_string())
                        .add_param("max_id", (prev_id - 1).to_string())
                },
            };
            let request = RequestBuilder::new(Method::GET, TWEET_FAV_LIST_API)
                .with_query_params(&params)
                .request_keys(conn_token, Some(resource_token));
            let rt = Runtime::new().unwrap();
            let json: Response<serde_json::Value> = rt.block_on(response_json(request))?;
            let fav_list = json.response.as_array().ok_or("invalid response")?;
            let remain_quota = json.rate_limit_status.remaining;
            if fav_list.is_empty() {
                println!("all done!");
                break;
            }
            if remain_quota > 0 {
                println!("fetched {} favorited tweets, remain quota: {}", &fav_list.len(), remain_quota);
            } else {
                /*
                rate limit: 75 requests / 15 min
                https://developer.twitter.com/en/docs/twitter-api/v1/tweets/post-and-engage/api-reference/get-favorites-list
                 */
                println!("no quota, sleep 15 minutes to recover...");
                thread::sleep(Duration::from_secs(900));
            }
            prev_id = fav_list[fav_list.len() - 1]["id"].as_u64().ok_or("can't parse tweet id")?;
            let _ = &self.update_images(fav_list.to_vec(), save_path)?;
            if !scanall {
                break;
            }
        }
        Ok(())
    }

}

fn find_max_bitrate_url(variants: &[serde_json::value::Value]) -> Result<String, Box<dyn std::error::Error>> {
    let mut max_bitrate = 0;
    let mut max_idx = 0;
    for (i, variant) in variants.iter().enumerate() {
        if variant["content_type"].as_str() != Some("application/x-mpegURL") && max_bitrate < variant["bitrate"].as_u64().unwrap_or(0) {
            max_idx = i;
            max_bitrate = variant["bitrate"].as_u64().unwrap_or(0);
        }
    }
    Ok(variants[max_idx]["url"].as_str().expect("url convert error").to_string())
}

async fn save_content(conn_token: &egg_mode::KeyPair, resource_token: &egg_mode::KeyPair, img_url: &str, save_path: &Path) -> Result<(), Box<dyn std::error::Error>> {

    let fname = img_url.rsplit('/').next().ok_or("path error")?.split('?').next().ok_or("url parse error")?;
    if save_path.join(fname).exists() {
        return Ok(());
    }
    let url = img_url.to_owned();
    let img_format = url.rsplit('.').next().ok_or("url error")?.split('?').next().ok_or("url parse error")?;
    let request = match img_format {
        "mp4" => {
            RequestBuilder::new(Method::GET, img_url)
                .request_keys(conn_token, Some(resource_token))
        },
        _ => {
            let params = ParamList::new()
                .add_param("format", img_format.to_string())
                .add_param("name", "orig");
            RequestBuilder::new(Method::GET, img_url)
                .with_query_params(&params)
                .request_keys(conn_token, Some(resource_token))
        }
    };
    let bytes: Vec<u8> = response_raw_bytes(request).await?.1;

    let fname = img_url.rsplit('/').next().ok_or("path error")?.split('?').next().ok_or("url parse error")?;
    create_dir_all(save_path)?;
    let mut buffer = File::create(save_path.join(fname))?;
    buffer.write_all(&bytes)?;
    Ok(())
}

fn main() {
    let cli = Cli::parse();

    match cli.operation {
        Operation::Download => {
            let download_ctl = TweetsImagesDownloadController::from_envvar();
            let save_path = match cli.save_path.as_deref() {
                Some(path) => {
                    println!("path for image saving: {}", path.display());
                    path
                },
                _ => {
                    Path::new("media")
                },
            };
            download_ctl.download_favorited_images(cli.scanall, save_path).unwrap();
        },
    }
}
