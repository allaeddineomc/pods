use std::cell::{Cell, RefCell};
use std::fmt;
use std::str::FromStr;

use futures::{Future, TryFutureExt};
use gettextrs::gettext;
use gtk::glib::{self, clone, WeakRef};
use gtk::prelude::{ObjectExt, StaticType, ToValue};
use gtk::subclass::prelude::*;
use once_cell::sync::Lazy;
use once_cell::unsync::OnceCell;

use crate::{api, model, utils, PODMAN};

#[derive(Debug, Copy, Clone, PartialEq, Eq, glib::Enum)]
#[enum_type(name = "ContainerStatus")]
pub(crate) enum Status {
    Configured,
    Created,
    Dead,
    Exited,
    Paused,
    Removing,
    Restarting,
    Running,
    Stopped,
    Stopping,
    Unknown,
}

impl Default for Status {
    fn default() -> Self {
        Self::Unknown
    }
}

impl FromStr for Status {
    type Err = Self;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "configured" => Self::Configured,
            "created" => Self::Created,
            "dead" => Self::Dead,
            "exited" => Self::Exited,
            "paused" => Self::Paused,
            "removing" => Self::Removing,
            "restarting" => Self::Restarting,
            "running" => Self::Running,
            "stopped" => Self::Stopped,
            "stopping" => Self::Stopping,
            _ => return Err(Self::Unknown),
        })
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Configured => gettext("Configured"),
                Self::Created => gettext("Created"),
                Self::Dead => gettext("Dead"),
                Self::Exited => gettext("Exited"),
                Self::Paused => gettext("Paused"),
                Self::Removing => gettext("Removing"),
                Self::Restarting => gettext("Restarting"),
                Self::Running => gettext("Running"),
                Self::Stopped => gettext("Stopped"),
                Self::Stopping => gettext("Stopping"),
                Self::Unknown => gettext("Unknown"),
            }
        )
    }
}

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub(crate) struct Container {
        pub(super) action_ongoing: Cell<bool>,
        pub(super) id: OnceCell<String>,
        pub(super) image: WeakRef<model::Image>,
        pub(super) image_id: OnceCell<String>,
        pub(super) image_name: RefCell<Option<String>>,
        pub(super) name: RefCell<Option<String>>,
        pub(super) status: Cell<Status>,
        pub(super) user: RefCell<Option<String>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Container {
        const NAME: &'static str = "Container";
        type Type = super::Container;
        type ParentType = glib::Object;
    }

    impl ObjectImpl for Container {
        fn properties() -> &'static [glib::ParamSpec] {
            static PROPERTIES: Lazy<Vec<glib::ParamSpec>> = Lazy::new(|| {
                vec![
                    glib::ParamSpecBoolean::new(
                        "action-ongoing",
                        "Action Ongoing",
                        "Whether an action (starting, stopping, etc.) is currently ongoing",
                        false,
                        glib::ParamFlags::READWRITE,
                    ),
                    glib::ParamSpecString::new(
                        "id",
                        "Id",
                        "The id of this container",
                        Option::default(),
                        glib::ParamFlags::READWRITE | glib::ParamFlags::CONSTRUCT_ONLY,
                    ),
                    glib::ParamSpecObject::new(
                        "image",
                        "Image",
                        "The image of this container",
                        model::Image::static_type(),
                        glib::ParamFlags::READWRITE | glib::ParamFlags::EXPLICIT_NOTIFY,
                    ),
                    glib::ParamSpecString::new(
                        "image-id",
                        "Image Id",
                        "The image id of this container",
                        Option::default(),
                        glib::ParamFlags::READWRITE | glib::ParamFlags::CONSTRUCT_ONLY,
                    ),
                    glib::ParamSpecString::new(
                        "image-name",
                        "Image Name",
                        "The name of the image of this container",
                        Option::default(),
                        glib::ParamFlags::READWRITE
                            | glib::ParamFlags::CONSTRUCT
                            | glib::ParamFlags::EXPLICIT_NOTIFY,
                    ),
                    glib::ParamSpecString::new(
                        "name",
                        "Name",
                        "The name of this container",
                        Option::default(),
                        glib::ParamFlags::READWRITE
                            | glib::ParamFlags::CONSTRUCT
                            | glib::ParamFlags::EXPLICIT_NOTIFY,
                    ),
                    glib::ParamSpecEnum::new(
                        "status",
                        "Status",
                        "The status of this container",
                        Status::static_type(),
                        Status::default() as i32,
                        glib::ParamFlags::READWRITE | glib::ParamFlags::EXPLICIT_NOTIFY,
                    ),
                    glib::ParamSpecString::new(
                        "user",
                        "User",
                        "The user of this container",
                        Option::default(),
                        glib::ParamFlags::READWRITE
                            | glib::ParamFlags::CONSTRUCT
                            | glib::ParamFlags::EXPLICIT_NOTIFY,
                    ),
                ]
            });
            PROPERTIES.as_ref()
        }

        fn set_property(
            &self,
            obj: &Self::Type,
            _id: usize,
            value: &glib::Value,
            pspec: &glib::ParamSpec,
        ) {
            match pspec.name() {
                "action-ongoing" => obj.set_action_ongoing(value.get().unwrap()),
                "id" => self.id.set(value.get().unwrap()).unwrap(),
                "image" => obj.set_image(value.get().unwrap()),
                "image-id" => self.image_id.set(value.get().unwrap()).unwrap(),
                "image-name" => obj.set_image_name(value.get().unwrap()),
                "name" => obj.set_name(value.get().unwrap()),
                "status" => obj.set_status(value.get().unwrap()),
                "user" => obj.set_user(value.get().unwrap()),
                _ => unimplemented!(),
            }
        }

        fn property(&self, obj: &Self::Type, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            match pspec.name() {
                "action-ongoing" => obj.action_ongoing().to_value(),
                "id" => obj.id().to_value(),
                "image" => obj.image().to_value(),
                "image-id" => obj.image_id().to_value(),
                "image-name" => obj.image_name().to_value(),
                "name" => obj.name().to_value(),
                "status" => obj.status().to_value(),
                "user" => obj.user().to_value(),
                _ => unimplemented!(),
            }
        }
    }
}

glib::wrapper! {
    pub(crate) struct Container(ObjectSubclass<imp::Container>);
}

impl From<api::LibpodContainerInspectResponse> for Container {
    fn from(inspect_response: api::LibpodContainerInspectResponse) -> Self {
        glib::Object::new(&[
            ("id", &inspect_response.id),
            ("image-id", &inspect_response.image),
            ("image-name", &inspect_response.image_name),
            ("name", &inspect_response.name),
            ("status", &status(inspect_response.state)),
            (
                "user",
                &inspect_response.config.and_then(|config| config.user),
            ),
        ])
        .expect("Failed to create Container")
    }
}

impl Container {
    pub(crate) fn update(&self, inspect_response: api::LibpodContainerInspectResponse) {
        self.set_action_ongoing(false);
        self.set_image_name(inspect_response.image_name);
        self.set_name(inspect_response.name);
        self.set_status(status(inspect_response.state));
        self.set_user(inspect_response.config.and_then(|config| config.user));
    }

    pub(crate) fn action_ongoing(&self) -> bool {
        self.imp().action_ongoing.get()
    }

    pub(crate) fn set_action_ongoing(&self, value: bool) {
        if self.action_ongoing() == value {
            return;
        }
        self.imp().action_ongoing.replace(value);
        self.notify("action-ongoing");
    }

    pub(crate) fn id(&self) -> Option<&str> {
        self.imp().id.get().map(String::as_str)
    }

    pub(crate) fn image(&self) -> Option<model::Image> {
        self.imp().image.upgrade()
    }

    pub(crate) fn set_image(&self, value: Option<&model::Image>) {
        if self.image().as_ref() == value {
            return;
        }
        self.imp().image.set(value);
        self.notify("image");
    }

    pub(crate) fn image_id(&self) -> Option<&str> {
        self.imp().image_id.get().map(String::as_str)
    }

    pub(crate) fn image_name(&self) -> Option<String> {
        self.imp().image_name.borrow().clone()
    }

    pub(crate) fn set_image_name(&self, value: Option<String>) {
        if self.image_name() == value {
            return;
        }
        self.imp().image_name.replace(value);
        self.notify("image-name");
    }

    pub(crate) fn name(&self) -> Option<String> {
        self.imp().name.borrow().clone()
    }

    pub(crate) fn set_name(&self, value: Option<String>) {
        if self.name() == value {
            return;
        }
        self.imp().name.replace(value);
        self.notify("name");
    }

    pub(crate) fn status(&self) -> Status {
        self.imp().status.get()
    }

    pub(crate) fn set_status(&self, value: Status) {
        if self.status() == value {
            return;
        }
        self.imp().status.set(value);
        self.notify("status");
    }

    pub(crate) fn user(&self) -> Option<String> {
        self.imp().user.borrow().to_owned()
    }

    pub(crate) fn set_user(&self, value: Option<String>) {
        if self.user() == value {
            return;
        }
        self.imp().user.replace(value);
        self.notify("user");
    }
}

impl Container {
    fn action<Fut, FutOp, ErrOp>(&self, fut_op: FutOp, err_op: ErrOp)
    where
        Fut: Future<Output = api::Result<()>> + Send,
        FutOp: FnOnce(api::Container<'static>) -> Fut + Send + 'static,
        ErrOp: FnOnce(api::Error) + 'static,
    {
        if self.action_ongoing() {
            return;
        }

        // This will be either set back to `false` in `Self::update` or in case of an error.
        self.set_action_ongoing(true);

        let container = api::Container::new(&*PODMAN, self.id().unwrap_or_default());
        utils::do_async(
            async move { fut_op(container).await },
            clone!(@weak self as obj => move |result| match result {
                Ok(_) => {
                    log::info!("Container <{}>: Action is finished", obj.id().unwrap_or_default());
                }
                Err(e) => {
                    obj.set_action_ongoing(false);
                    err_op(e)
                }
            }),
        );
    }

    pub(crate) fn start<F>(&self, err_op: F)
    where
        F: FnOnce(api::Error) + 'static,
    {
        log::info!("Container <{}>: Starting…'", self.id().unwrap_or_default());
        self.action(
            |container| async move { container.start(None).await },
            clone!(@weak self as obj => move |e| {
                log::error!(
                    "Container <{}>: Error while starting: {}",
                    obj.id().unwrap_or_default(),
                    e
                );
                err_op(e);
            }),
        );
    }

    pub(crate) fn stop<F>(&self, err_op: F)
    where
        F: FnOnce(api::Error) + 'static,
    {
        log::info!("Container <{}>: Stopping…'", self.id().unwrap_or_default());
        self.action(
            |container| async move { container.stop(&Default::default()).await },
            clone!(@weak self as obj => move |e| {
                log::error!(
                    "Container <{}>: Error while stopping: {}",
                    obj.id().unwrap_or_default(),
                    e
                );
                err_op(e);
            }),
        );
    }

    pub(crate) fn force_stop<F>(&self, err_op: F)
    where
        F: FnOnce(api::Error) + 'static,
    {
        log::info!(
            "Container <{}>: Force stopping…'",
            self.id().unwrap_or_default()
        );
        self.action(
            |container| async move { container.kill().await },
            clone!(@weak self as obj => move |e| {
                log::error!(
                    "Container <{}>: Error while force stopping: {}",
                    obj.id().unwrap_or_default(),
                    e
                );
                err_op(e);
            }),
        );
    }

    pub(crate) fn restart<F>(&self, err_op: F)
    where
        F: FnOnce(api::Error) + 'static,
    {
        log::info!(
            "Container <{}>: Restarting…'",
            self.id().unwrap_or_default()
        );
        self.action(
            |container| async move { container.restart().await },
            clone!(@weak self as obj => move |e| {
                log::error!(
                    "Container <{}>: Error while restarting: {}",
                    obj.id().unwrap_or_default(),
                    e
                );
                err_op(e);
            }),
        );
    }

    pub(crate) fn force_restart<F>(&self, err_op: F)
    where
        F: FnOnce(api::Error) + 'static,
    {
        log::info!(
            "Container <{}>: Force restarting…'",
            self.id().unwrap_or_default()
        );
        self.action(
            |container| async move { container.kill().and_then(|_| container.start(None)).await },
            clone!(@weak self as obj => move |e| {
                log::error!(
                    "Container <{}>: Error while force restarting: {}",
                    obj.id().unwrap_or_default(),
                    e
                );
                err_op(e);
            }),
        );
    }

    pub(crate) fn pause<F>(&self, err_op: F)
    where
        F: FnOnce(api::Error) + 'static,
    {
        log::info!("Container <{}>: Pausing…'", self.id().unwrap_or_default());
        self.action(
            |container| async move { container.pause().await },
            clone!(@weak self as obj => move |e| {
                log::error!(
                    "Container <{}>: Error while pausing: {}",
                    obj.id().unwrap_or_default(),
                    e
                );
                err_op(e);
            }),
        );
    }

    pub(crate) fn resume<F>(&self, err_op: F)
    where
        F: FnOnce(api::Error) + 'static,
    {
        log::info!("Container <{}>: Resuming…'", self.id().unwrap_or_default());
        self.action(
            |container| async move { container.unpause().await },
            clone!(@weak self as obj => move |e| {
                log::error!(
                    "Container <{}>: Error while resuming: {}",
                    obj.id().unwrap_or_default(),
                    e
                );
                err_op(e);
            }),
        );
    }

    pub(crate) fn commit<F>(&self, err_op: F)
    where
        F: FnOnce(api::Error) + 'static,
    {
        log::info!(
            "Container <{}>: Committing…'",
            self.id().unwrap_or_default()
        );
        self.action(
            |container| async move { container.commit(&Default::default()).await },
            clone!(@weak self as obj => move |e| {
                log::error!(
                    "Container <{}>: Error while committing: {}",
                    obj.id().unwrap_or_default(),
                    e
                );
                err_op(e);
            }),
        );
    }

    pub(crate) fn delete<F>(&self, err_op: F)
    where
        F: FnOnce(api::Error) + 'static,
    {
        log::info!("Container <{}>: Deleting…'", self.id().unwrap_or_default());
        self.action(
            |container| async move { container.delete(&Default::default()).await },
            clone!(@weak self as obj => move |e| {
                log::error!(
                    "Container <{}>: Error while deleting: {}",
                    obj.id().unwrap_or_default(),
                    e
                );
                err_op(e);
            }),
        );
    }

    pub(crate) fn force_delete<F>(&self, err_op: F)
    where
        F: FnOnce(api::Error) + 'static,
    {
        log::info!(
            "Container <{}>: Force deleting…'",
            self.id().unwrap_or_default()
        );
        self.action(
            |container| async move {
                let delete_opts = Default::default();
                container
                    .stop(&Default::default())
                    .and_then(|_| container.delete(&delete_opts))
                    .await
            },
            clone!(@weak self as obj => move |e| {
                log::error!(
                    "Container <{}>: Error while force deleting: {}",
                    obj.id().unwrap_or_default(),
                    e
                );
                err_op(e);
            }),
        );
    }
}

fn status(state: Option<api::InspectContainerState>) -> Status {
    state
        .and_then(|state| state.status)
        .map_or_else(Status::default, |s| match Status::from_str(&s) {
            Ok(status) => status,
            Err(status) => {
                log::warn!("Unknown container status: {s}");
                status
            }
        })
}
