use crate::{views::{friends_view::{FriendsViewBarPage, FriendsViewBarStatusFilter}, game_view::{GameViewBarGenre, GameViewBarPlatform}}, translation_manager::TranslationManager};

pub trait EnumToString<T> {
    fn get_string(&self, variant: &mut T) -> String;
    fn get_string_nonmut(&self, variant: &T) -> String;
}

impl EnumToString<FriendsViewBarPage> for TranslationManager {
    fn get_string_nonmut(&self, variant: &FriendsViewBarPage) -> String {
        match variant {
            FriendsViewBarPage::Online => self.localization.friends_view.toolbar.online.clone(),
            FriendsViewBarPage::All => self.localization.friends_view.toolbar.all.clone(),
            FriendsViewBarPage::Pending => self.localization.friends_view.toolbar.pending.clone(),
            FriendsViewBarPage::Blocked => self.localization.friends_view.toolbar.blocked.clone(),
        }
    }
    fn get_string(&self, variant: &mut FriendsViewBarPage) -> String {
        self.get_string_nonmut(variant)
    }
}

impl EnumToString<FriendsViewBarStatusFilter> for TranslationManager {
    fn get_string_nonmut(&self, variant: &FriendsViewBarStatusFilter) -> String {
        match variant {
            FriendsViewBarStatusFilter::Name => self.localization.friends_view.toolbar.filter_options.name.clone(),
            FriendsViewBarStatusFilter::Game => self.localization.friends_view.toolbar.filter_options.game.clone(),
        }
    }
    fn get_string(&self, variant: &mut FriendsViewBarStatusFilter) -> String {
        self.get_string_nonmut(variant)
    }
}

impl EnumToString<GameViewBarGenre> for TranslationManager {
    fn get_string_nonmut(&self, variant: &GameViewBarGenre) -> String {
        match variant {
            GameViewBarGenre::AllGames => self.localization.games_view.toolbar.genre_options.all.clone(),
            GameViewBarGenre::Shooters => self.localization.games_view.toolbar.genre_options.shooter.clone(),
            GameViewBarGenre::Simulation => self.localization.games_view.toolbar.genre_options.simulation.clone(),
        }
    }
    fn get_string(&self, variant: &mut GameViewBarGenre) -> String {
        self.get_string_nonmut(variant)
    }
}

impl EnumToString<GameViewBarPlatform> for TranslationManager {
    fn get_string_nonmut(&self, variant: &GameViewBarPlatform) -> String {
        match variant {
            GameViewBarPlatform::AllPlatforms => self.localization.games_view.toolbar.platform_options.all.clone(),
            GameViewBarPlatform::Windows => self.localization.games_view.toolbar.platform_options.windows.clone(),
            GameViewBarPlatform::Mac => self.localization.games_view.toolbar.platform_options.mac.clone(),
        }
    }
    fn get_string(&self, variant: &mut GameViewBarPlatform) -> String {
        self.get_string_nonmut(variant)
    }
}