// TODO: 
// - Packaging up python lib
// - Logging
// - CI/CD
// - Tests
// - Documentation
// - RELEASE
// - Config for setting auth and url instead of using env vars

use std::{ffi::CStr, sync::Mutex, env, collections::HashMap, time::SystemTime};
use lazy_static::lazy_static;
use regex::Regex;
use serde::Deserialize;
use cache_control::CacheControl;
use std::time::Duration;

const URL: &str = "api.kokocares.org/keywords";
const CACHE_EXPIRATION_DEFAULT: Duration = Duration::from_secs(3600);

type KokoResult<T> = Result<T, KokoError>;

#[derive(Debug, Clone, Copy)]
pub enum KokoError {
    Padding,
    AuthOrUrlMissing,
    CacheRefreshRequestFailure,
    CacheResultParseFailure,
    ParseError,
}

#[derive(Deserialize, Debug)]
struct Keywords {
    pub keywords: Vec<String>,
    pub preprocess: String,
}

struct KeywordsCache {
    pub expires_at: SystemTime,
    pub keywords: Keywords,
}

#[derive(Deserialize, Debug)]
struct ApiResponse {
    pub regex: Keywords,
}

struct KokoKeywords {
    pub keywords: HashMap<String, KeywordsCache>,
    pub url: String,
}

impl KokoKeywords {
    pub fn new(url: String) -> Self {
        Self { keywords: HashMap::new(), url }
    }

    pub fn verify(&mut self, keyword: &str, filter: &str, version: Option<&str>) -> KokoResult<bool> {
        let cache_key = format!("{}{}", filter, version.unwrap_or_default());

        if let Some(keyword_cache) = self.keywords.get(&cache_key) {
            if SystemTime::now() < keyword_cache.expires_at  {
                let re = Regex::new(&keyword_cache.keywords.preprocess).unwrap();
                let keyword = re.replace_all(keyword, "");

                for re_keyword in &keyword_cache.keywords.keywords {
                    let re = Regex::new(re_keyword).unwrap();
                    if re.is_match(&keyword) {
                        return Ok(true);
                    }
                }

                return Ok(false);
            } else {
                self.load_cache(filter, version)?;
                self.verify(keyword, filter, version)
            }
        } else {
            self.load_cache(filter, version)?;
            self.verify(keyword, filter, version)
        }
    }

    pub fn load_cache(&mut self, filter: &str, version: Option<&str>) -> KokoResult<()> {
        let cache_key = format!("{}{}", filter, version.unwrap_or_default());

        println!("Loading cache for key '{}'", cache_key);

        let request = ureq::get(&self.url);

        let request = request.query("filter", filter);
        let request = if let Some(version) = version {
            request.query("version", version)
        } else {
            request
        };

        let response = request.call().map_err(|_| KokoError::ParseError)?;

        let expires_in = response.header("cache-control")
            .map(CacheControl::from_value)
            .flatten()
            .map(|cc| cc.max_age)
            .flatten()
            .unwrap_or(CACHE_EXPIRATION_DEFAULT);

        let api_response: ApiResponse = serde_json::from_reader(response.into_reader()).map_err(|_| KokoError::ParseError)?;
        let keywords_cache = KeywordsCache {
            keywords: api_response.regex,
            expires_at: SystemTime::now() + expires_in
        };
        self.keywords.insert(cache_key.to_string(), keywords_cache);

        Ok(())
    }
}

// #[cfg(test)]
// mod test {
//     use super::*;

//     #[test]
//     fn test_match_keyword() {
//         let x = KeywordMatcher { regex: RegexResponse {
//             keywords: vec!["blah".to_string()],
//             preprocess: "yes".to_string(),
//         }};

//         //assert!(x.match_keyword("yadiyada"));
//         assert!(!x.match_keyword("yadiyqweqweada"));
//     }
// }

lazy_static! {
    static ref MATCHER: Mutex<KokoResult<KokoKeywords>> =
        Mutex::new(get_url().map(KokoKeywords::new));
}


pub fn get_url() -> KokoResult<String> {
    match (env::var("KOKO_KEYWORDS_URL").ok(), env::var("KOKO_KEYWORDS_AUTH").ok()) {
        (Some(_), Some(_)) => Err(KokoError::AuthOrUrlMissing),
        (Some(url), None) => Ok(url),
        (None, Some(auth)) => Ok(format!("https://{}@{}", auth, URL)),
        (None, None) => Err(KokoError::AuthOrUrlMissing),
    }
}

fn koko_keywords_match_inner(input: &str, filter: &str, version: Option<&str>) -> KokoResult<bool> {
    MATCHER.lock().unwrap().as_mut().map_err(|e| e.clone())?
        .verify(input, filter, version)
}

#[no_mangle]
pub extern "C" fn koko_keywords_match(input: *const i8, filter: *const i8, version: *const i8,) -> isize {
    let input = str_from_c(input).expect("Input is required");
    let filter = str_from_c(filter).expect("Filter is required");
    let version = str_from_c(version);

    println!("Calling with {:?}, {:?}, {:?}", input, filter, version);

    let result = koko_keywords_match_inner(input, filter, version);
    println!("Result: {:?}", result);
    match result {
        Ok(r) => if r { 1 } else { 0 }
        Err(e) => -(e as isize),
    }
}

pub fn str_from_c<'a>(c_str: *const i8) -> Option<&'a str> {
    if c_str.is_null() {
        None
    } else {
        Some(
            unsafe { CStr::from_ptr(c_str) }
                .to_str().expect("Malformed UTF-8 string")
        )
    }
}
