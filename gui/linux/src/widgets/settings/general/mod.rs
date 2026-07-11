use std::rc::Rc;

use libadwaita::{ApplicationWindow, PreferencesGroup, PreferencesPage, SwitchRow, prelude::*};
use log::warn;

use crate::{services::autostart, widgets::settings::helper::show_error_dialog};

pub(crate) struct GeneralPage {
    page: PreferencesPage,
    autostart_row: SwitchRow,
}

impl GeneralPage {
    pub(crate) fn new(window: &ApplicationWindow) -> Rc<Self> {
        let autostart_row = SwitchRow::builder().title("Launch at login").build();
        match autostart::is_enabled() {
            Ok(enabled) => autostart_row.set_active(enabled),
            Err(e) => {
                warn!("autostart: cannot read entry ({e})");
                autostart_row.set_sensitive(false);
            },
        }

        let window = window.downgrade();
        autostart_row.connect_active_notify(move |row| {
            let actual = match autostart::is_enabled() {
                Ok(actual) => actual,
                Err(e) => {
                    warn!("autostart: cannot read entry ({e})");
                    return;
                },
            };
            let requested = row.is_active();
            // the failure revert re-enters here; matching reality means nothing to do
            if requested == actual {
                return;
            }
            let result = if requested {
                autostart::enable()
            } else {
                autostart::disable()
            };
            if let Err(e) = result {
                warn!("autostart: cannot update entry ({e})");
                if let Some(window) = window.upgrade() {
                    show_error_dialog(
                        &window,
                        "Autostart",
                        &format!("Could not update the autostart entry: {e}"),
                    );
                }
                row.set_active(actual);
            }
        });

        let group = PreferencesGroup::builder().title("Startup").build();
        group.add(&autostart_row);
        let page = PreferencesPage::new();
        page.add(&group);

        Rc::new(Self {
            page,
            autostart_row,
        })
    }

    pub(crate) fn widget(&self) -> &PreferencesPage {
        &self.page
    }

    pub(crate) fn refresh(&self) {
        match autostart::is_enabled() {
            Ok(enabled) => {
                self.autostart_row.set_sensitive(true);
                self.autostart_row.set_active(enabled);
            },
            Err(e) => {
                warn!("autostart: cannot read entry ({e})");
                self.autostart_row.set_sensitive(false);
            },
        }
    }
}
