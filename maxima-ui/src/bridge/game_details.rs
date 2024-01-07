use anyhow::{Ok, Result};
use egui::Context;
use std::sync::{mpsc::Sender, Arc};
use tokio::sync::Mutex;

use crate::{
    interact_thread::{InteractThreadGameDetailsResponse, MaximaLibResponse},
    util::markdown::html_to_easymark,
    GameDetails,
};
use maxima::core::{
    service_layer::{
        ServiceGameSystemRequirements, ServiceGameSystemRequirementsRequestBuilder,
        SERVICE_REQUEST_GAMESYSTEMREQUIREMENTS,
    },
    Maxima,
};

pub async fn game_details_request(
    maxima_arc: Arc<Mutex<Maxima>>,
    slug: String,
    channel: Sender<MaximaLibResponse>,
    ctx: &Context,
) -> Result<()> {
    let maxima = maxima_arc.lock().await;

    let yeah = maxima.service_layer().request(
        SERVICE_REQUEST_GAMESYSTEMREQUIREMENTS,
        ServiceGameSystemRequirementsRequestBuilder::default()
            .slug(slug.clone())
            .locale(maxima.locale().short_str().to_owned())
            .build()?,
    );
    let yeah: ServiceGameSystemRequirements = yeah.await?;

    //TODO: parse async

    let min = html_to_easymark(yeah.system_requirements()[0].minimum());
    let rec = html_to_easymark(yeah.system_requirements()[0].recommended());

    let res = MaximaLibResponse::GameDetailsResponse(InteractThreadGameDetailsResponse {
        slug: slug.clone(),
        response: Ok(GameDetails {
            time: 0,
            achievements_unlocked: 0,
            achievements_total: 12,
            path: String::new(),
            system_requirements_min: min,
            system_requirements_rec: rec,
        }),
    });
    let _ = channel.send(res);
    egui::Context::request_repaint(&ctx);
    Ok(())
}
