use serde::Serialize;
use std::error::Error as _;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Msg(String),
    #[error("{0}")]
    Http(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Url(#[from] url::ParseError),
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        let mut s = e.to_string();
        let mut src = e.source();
        while let Some(c) = src {
            let n = c.to_string();
            if !s.contains(&n) {
                s.push_str(" → ");
                s.push_str(&n);
            }
            src = c.source();
        }
        if s.contains("10061")
            || s.contains("积极拒绝")
            || s.contains("tunnel error")
            || s.contains("failed to create underlying connection")
        {
            return AppError::Http(format!("连不上学校站点（连接被拒绝）。{s}"));
        }
        if e.is_timeout()
            || s.contains("timed out")
            || s.contains("timeout")
            || s.contains("operation timed out")
        {
            return AppError::Http(
                "连学校认证站超时。校外请确认浏览器能打开 pass.neu.edu.cn，或先连校园网 / VPN。"
                    .into(),
            );
        }
        AppError::Http(s)
    }
}

impl From<String> for AppError {
    fn from(s: String) -> Self {
        AppError::Msg(s)
    }
}

impl From<&str> for AppError {
    fn from(s: &str) -> Self {
        AppError::Msg(s.to_string())
    }
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
