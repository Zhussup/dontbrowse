use std::sync::Arc;

use crate::images::HalfblockImage;
use crate::media::PlayerEvent;
use crate::model::{Article, FeedPage, FeedTarget};

#[derive(Debug)]
pub enum AppEvent {
    FeedLoaded {
        job: u64,
        target: FeedTarget,
        page: FeedPage,
    },
    ArticleLoaded {
        job: u64,
        article: Box<Article>,
    },
    Progress {
        job: u64,
        stage: String,
    },
    Failed {
        job: u64,
        message: String,
    },
    ImageReady {
        key: String,
        image: Arc<HalfblockImage>,
    },
    ImageFailed {
        key: String,
    },
    Player(PlayerEvent),
}
