use urobotics::app::adhoc_app;

fn video_app() {}

adhoc_app!(pub(super) VideoTestApp, "video", "Video stuff", video_app);
