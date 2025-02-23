use std::cell::Cell;
use std::cell::RefCell;
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

use futures::Future;
use gettextrs::gettext;
use gtk::glib;
use gtk::glib::clone;
use gtk::glib::subclass::Signal;
use gtk::glib::WeakRef;
use gtk::prelude::ObjectExt;
use gtk::prelude::StaticType;
use gtk::prelude::ToValue;
use gtk::subclass::prelude::*;
use once_cell::sync::Lazy;
use once_cell::unsync::OnceCell;

use crate::model;
use crate::monad_boxed_type;
use crate::podman;
use crate::utils;

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, glib::Enum)]
#[enum_type(name = "ContainerStatus")]
pub(crate) enum Status {
    Created,
    Dead,
    Exited,
    Paused,
    Removing,
    Restarting,
    Running,
    Stopped,
    Stopping,
    #[default]
    Unknown,
}

impl FromStr for Status {
    type Err = Self;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
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

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, glib::Enum)]
#[enum_type(name = "ContainerHealthStatus")]
pub(crate) enum HealthStatus {
    Starting,
    Healthy,
    Unhealthy,
    Unconfigured,
    #[default]
    Unknown,
}

impl FromStr for HealthStatus {
    type Err = Self;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "starting" => Self::Starting,
            "healthy" => Self::Healthy,
            "unhealthy" => Self::Unhealthy,
            "" => Self::Unconfigured,
            _ => return Err(Self::Unknown),
        })
    }
}

impl fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Starting => gettext("Checking"),
                Self::Healthy => gettext("Healthy"),
                Self::Unhealthy => gettext("Unhealthy"),
                Self::Unconfigured => gettext("Unconfigured"),
                Self::Unknown => gettext("Unknown"),
            }
        )
    }
}

monad_boxed_type!(pub(crate) BoxedContainerStats(podman::models::ContainerStats) impls Debug, PartialEq is nullable);

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub(crate) struct Container {
        pub(super) container_list: WeakRef<model::ContainerList>,

        pub(super) action_ongoing: Cell<bool>,

        pub(super) created: OnceCell<i64>,
        pub(super) health_status: Cell<HealthStatus>,
        pub(super) id: OnceCell<String>,
        pub(super) image: WeakRef<model::Image>,
        pub(super) image_id: OnceCell<String>,
        pub(super) image_name: RefCell<Option<String>>,
        pub(super) name: RefCell<String>,
        pub(super) pod: WeakRef<model::Pod>,
        pub(super) pod_id: OnceCell<Option<String>>,
        pub(super) port_bindings: OnceCell<utils::BoxedStringVec>,
        pub(super) stats: RefCell<Option<BoxedContainerStats>>,
        pub(super) status: Cell<Status>,
        pub(super) up_since: Cell<i64>,

        pub(super) data: OnceCell<model::ContainerData>,
        pub(super) can_inspect: Cell<bool>,

        pub(super) selected: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Container {
        const NAME: &'static str = "Container";
        type Type = super::Container;
        type Interfaces = (model::Selectable,);
    }

    impl ObjectImpl for Container {
        fn signals() -> &'static [Signal] {
            static SIGNALS: Lazy<Vec<Signal>> = Lazy::new(|| {
                vec![Signal::builder("deleted", &[], <()>::static_type().into()).build()]
            });
            SIGNALS.as_ref()
        }

        fn properties() -> &'static [glib::ParamSpec] {
            static PROPERTIES: Lazy<Vec<glib::ParamSpec>> = Lazy::new(|| {
                vec![
                    glib::ParamSpecObject::new(
                        "container-list",
                        "Container List",
                        "The parent container list",
                        model::ContainerList::static_type(),
                        glib::ParamFlags::READWRITE | glib::ParamFlags::CONSTRUCT_ONLY,
                    ),
                    glib::ParamSpecBoolean::new(
                        "action-ongoing",
                        "Action Ongoing",
                        "Whether an action (starting, stopping, etc.) is currently ongoing",
                        false,
                        glib::ParamFlags::READWRITE | glib::ParamFlags::EXPLICIT_NOTIFY,
                    ),
                    glib::ParamSpecBoolean::new(
                        "deleted",
                        "Deleted",
                        "Whether this container is deleted",
                        false,
                        glib::ParamFlags::READWRITE | glib::ParamFlags::EXPLICIT_NOTIFY,
                    ),
                    glib::ParamSpecInt64::new(
                        "created",
                        "Created",
                        "The time when this container was created",
                        i64::MIN,
                        i64::MAX,
                        0,
                        glib::ParamFlags::READWRITE | glib::ParamFlags::CONSTRUCT_ONLY,
                    ),
                    glib::ParamSpecEnum::new(
                        "health-status",
                        "Health Status",
                        "The status of this container",
                        HealthStatus::static_type(),
                        HealthStatus::default() as i32,
                        glib::ParamFlags::READWRITE
                            | glib::ParamFlags::CONSTRUCT
                            | glib::ParamFlags::EXPLICIT_NOTIFY,
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
                    glib::ParamSpecObject::new(
                        "pod",
                        "Pod",
                        "The potential pod of this container",
                        model::Pod::static_type(),
                        glib::ParamFlags::READWRITE | glib::ParamFlags::EXPLICIT_NOTIFY,
                    ),
                    glib::ParamSpecString::new(
                        "pod-id",
                        "Pod Id",
                        "The pod id of this container",
                        Option::default(),
                        glib::ParamFlags::READWRITE | glib::ParamFlags::CONSTRUCT_ONLY,
                    ),
                    glib::ParamSpecBoxed::new(
                        "port-bindings",
                        "Port Bindings",
                        "The port bindings of this container",
                        utils::BoxedStringVec::static_type(),
                        glib::ParamFlags::READWRITE | glib::ParamFlags::CONSTRUCT_ONLY,
                    ),
                    glib::ParamSpecBoxed::new(
                        "stats",
                        "Stats",
                        "The statistics of this container",
                        BoxedContainerStats::static_type(),
                        glib::ParamFlags::READWRITE | glib::ParamFlags::EXPLICIT_NOTIFY,
                    ),
                    glib::ParamSpecEnum::new(
                        "status",
                        "Status",
                        "The status of this container",
                        Status::static_type(),
                        Status::default() as i32,
                        glib::ParamFlags::READWRITE
                            | glib::ParamFlags::CONSTRUCT
                            | glib::ParamFlags::EXPLICIT_NOTIFY,
                    ),
                    glib::ParamSpecInt64::new(
                        "up-since",
                        "Up Since",
                        "The time since the container is running",
                        i64::MIN,
                        i64::MAX,
                        0,
                        glib::ParamFlags::READWRITE
                            | glib::ParamFlags::CONSTRUCT
                            | glib::ParamFlags::EXPLICIT_NOTIFY,
                    ),
                    glib::ParamSpecObject::new(
                        "data",
                        "Data",
                        "the data of the image",
                        model::ContainerData::static_type(),
                        glib::ParamFlags::READABLE,
                    ),
                    glib::ParamSpecBoolean::new(
                        "selected",
                        "Selected",
                        "Whether this container is selected",
                        false,
                        glib::ParamFlags::READWRITE,
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
                "container-list" => self.container_list.set(value.get().unwrap()),
                "action-ongoing" => obj.set_action_ongoing(value.get().unwrap()),
                "created" => self.created.set(value.get().unwrap()).unwrap(),
                "health-status" => obj.set_health_status(value.get().unwrap()),
                "id" => self.id.set(value.get().unwrap()).unwrap(),
                "image" => obj.set_image(value.get().unwrap()),
                "image-id" => self.image_id.set(value.get().unwrap()).unwrap(),
                "image-name" => obj.set_image_name(value.get().unwrap()),
                "pod" => obj.set_pod(value.get().unwrap()),
                "pod-id" => self.pod_id.set(value.get().unwrap()).unwrap(),
                "name" => obj.set_name(value.get().unwrap()),
                "port-bindings" => self.port_bindings.set(value.get().unwrap()).unwrap(),
                "stats" => obj.set_stats(value.get().unwrap()),
                "status" => obj.set_status(value.get().unwrap()),
                "up-since" => obj.set_up_since(value.get().unwrap()),
                "selected" => self.selected.set(value.get().unwrap()),
                _ => unimplemented!(),
            }
        }

        fn property(&self, obj: &Self::Type, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            match pspec.name() {
                "container-list" => obj.container_list().to_value(),
                "action-ongoing" => obj.action_ongoing().to_value(),
                "created" => obj.created().to_value(),
                "health-status" => obj.health_status().to_value(),
                "id" => obj.id().to_value(),
                "image" => obj.image().to_value(),
                "image-id" => obj.image_id().to_value(),
                "image-name" => obj.image_name().to_value(),
                "name" => obj.name().to_value(),
                "pod" => obj.pod().to_value(),
                "pod-id" => obj.pod_id().to_value(),
                "port-bindings" => obj.port_bindings().to_value(),
                "stats" => obj.stats().to_value(),
                "status" => obj.status().to_value(),
                "up-since" => obj.up_since().to_value(),
                "data" => obj.data().to_value(),
                "selected" => self.selected.get().to_value(),
                _ => unimplemented!(),
            }
        }

        fn constructed(&self, obj: &Self::Type) {
            self.parent_constructed(obj);
            self.can_inspect.set(true);
        }
    }
}

glib::wrapper! {
    pub(crate) struct Container(ObjectSubclass<imp::Container>) @implements model::Selectable;
}

impl Container {
    pub(crate) fn new(
        container_list: &model::ContainerList,
        list_container: podman::models::ListContainer,
    ) -> Self {
        glib::Object::new(&[
            ("container-list", container_list),
            (
                "created",
                &list_container.created.map(|dt| dt.timestamp()).unwrap_or(0),
            ),
            (
                "health-status",
                &health_status(list_container.status.as_deref()),
            ),
            ("id", &list_container.id),
            ("image-id", &list_container.image_id),
            ("image-name", &list_container.image),
            ("name", &list_container.names.unwrap()[0]),
            ("pod-id", &list_container.pod),
            (
                "port-bindings",
                &utils::BoxedStringVec::from(
                    list_container
                        .ports
                        .map(|mappings| {
                            mappings
                                .into_iter()
                                .map(|host_port| {
                                    format!(
                                        "{}:{}",
                                        {
                                            let ip = host_port.host_ip.unwrap_or_default();
                                            if ip.is_empty() {
                                                "127.0.0.1".to_string()
                                            } else {
                                                ip
                                            }
                                        },
                                        host_port.host_port.unwrap()
                                    )
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default(),
                ),
            ),
            ("status", &status(list_container.state.as_deref())),
            ("up-since", &list_container.started_at.unwrap()),
        ])
        .expect("Failed to create Container")
    }

    pub(crate) fn update(&self, list_container: podman::models::ListContainer) {
        self.set_action_ongoing(false);
        self.set_health_status(health_status(list_container.status.as_deref()));
        self.set_image_name(list_container.image);
        self.set_name(list_container.names.unwrap()[0].clone());
        self.set_status(status(list_container.state.as_deref()));
        self.set_up_since(list_container.started_at.unwrap());
    }

    pub(crate) fn container_list(&self) -> Option<model::ContainerList> {
        self.imp().container_list.upgrade()
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

    pub(crate) fn created(&self) -> i64 {
        *self.imp().created.get().unwrap()
    }

    pub(crate) fn health_status(&self) -> HealthStatus {
        self.imp().health_status.get()
    }

    pub(crate) fn set_health_status(&self, value: HealthStatus) {
        if self.health_status() == value {
            return;
        }
        self.imp().health_status.set(value);
        self.notify("health-status");
    }

    pub(crate) fn id(&self) -> &str {
        self.imp().id.get().unwrap()
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

    pub(crate) fn name(&self) -> String {
        self.imp().name.borrow().clone()
    }

    pub(crate) fn set_name(&self, value: String) {
        if self.name() == value {
            return;
        }
        self.imp().name.replace(value);
        self.notify("name");
    }

    pub(crate) fn pod(&self) -> Option<model::Pod> {
        self.imp().pod.upgrade()
    }

    pub(crate) fn set_pod(&self, value: Option<&model::Pod>) {
        if self.pod().as_ref() == value {
            return;
        }
        if let Some(pod) = value {
            pod.inspect_and_update();
        }
        self.imp().pod.set(value);
        self.notify("pod");
    }

    pub(crate) fn pod_id(&self) -> Option<&str> {
        self.imp()
            .pod_id
            .get()
            .unwrap()
            .as_ref()
            .map(String::as_str)
    }

    pub(crate) fn port_bindings(&self) -> &utils::BoxedStringVec {
        self.imp().port_bindings.get().unwrap()
    }

    pub(crate) fn stats(&self) -> Option<BoxedContainerStats> {
        self.imp().stats.borrow().clone()
    }

    pub fn set_stats(&self, value: Option<BoxedContainerStats>) {
        if self.stats() == value {
            return;
        }
        self.imp().stats.replace(value);
        self.notify("stats");
    }

    pub(crate) fn status(&self) -> Status {
        self.imp().status.get()
    }

    pub(crate) fn set_status(&self, value: Status) {
        if self.status() == value {
            return;
        }
        if let Some(pod) = self.pod() {
            pod.inspect_and_update();
        }
        self.imp().status.set(value);
        self.notify("status");
    }

    pub(crate) fn up_since(&self) -> i64 {
        self.imp().up_since.get()
    }

    pub(crate) fn set_up_since(&self, value: i64) {
        if self.up_since() == value {
            return;
        }
        self.imp().up_since.set(value);
        self.notify("up-since");
    }

    pub(crate) fn data(&self) -> Option<&model::ContainerData> {
        self.imp().data.get()
    }

    fn set_data(&self, data: podman::models::InspectContainerData) {
        if let Some(old) = self.data() {
            old.update(data);
            return;
        }
        self.imp()
            .data
            .set(model::ContainerData::from(data))
            .unwrap();
        self.notify("data");
    }

    pub(crate) fn inspect<F>(&self, op: F)
    where
        F: FnOnce(podman::Error) + 'static,
    {
        let imp = self.imp();
        if !imp.can_inspect.get() {
            return;
        }

        imp.can_inspect.set(false);

        utils::do_async(
            {
                let container = self.api().unwrap();
                async move { container.inspect().await }
            },
            clone!(@weak self as obj => move |result| {
                match result {
                    Ok(data) => obj.set_data(data),
                    Err(e) => {
                        log::error!("Error on inspecting container '{}': {e}", obj.id());
                        op(e);
                    },
                }
                obj.imp().can_inspect.set(true);
            }),
        );
    }
}

impl Container {
    fn action<Fut, FutOp, ResOp>(&self, name: &'static str, fut_op: FutOp, res_op: ResOp)
    where
        Fut: Future<Output = podman::Result<()>> + Send,
        FutOp: FnOnce(podman::api::Container) -> Fut + Send + 'static,
        ResOp: FnOnce(podman::Result<()>) + 'static,
    {
        if let Some(container) = self.api() {
            if self.action_ongoing() {
                return;
            }

            // This will be either set back to `false` in `Self::update` or in case of an error.
            self.set_action_ongoing(true);

            log::info!("Container <{}>: {name}…'", self.id());

            utils::do_async(
                async move { fut_op(container).await },
                clone!(@weak self as obj => move |result| {
                    match &result {
                        Ok(_) => {
                            log::info!(
                                "Container <{}>: {name} has finished",
                                obj.id()
                            );
                        }
                        Err(e) => {
                            log::error!(
                                "Container <{}>: Error while {name}: {e}",
                                obj.id(),
                            );
                            obj.set_action_ongoing(false);
                        }
                    }
                    res_op(result)
                }),
            );
        }
    }

    pub(crate) fn start<F>(&self, op: F)
    where
        F: FnOnce(podman::Result<()>) + 'static,
    {
        self.action(
            "starting",
            |container| async move { container.start(None).await },
            op,
        );
    }

    pub(crate) fn stop<F>(&self, force: bool, op: F)
    where
        F: FnOnce(podman::Result<()>) + 'static,
    {
        self.action(
            if force { "force stopping" } else { "stopping" },
            move |container| async move {
                if force {
                    container.kill().await
                } else {
                    container.stop(&Default::default()).await
                }
            },
            op,
        );
    }

    pub(crate) fn restart<F>(&self, force: bool, op: F)
    where
        F: FnOnce(podman::Result<()>) + 'static,
    {
        self.action(
            if force {
                "restarting"
            } else {
                "force restarting"
            },
            move |container| async move {
                if force {
                    container.restart_with_timeout(0).await
                } else {
                    container.restart().await
                }
            },
            op,
        );
    }

    pub(crate) fn pause<F>(&self, op: F)
    where
        F: FnOnce(podman::Result<()>) + 'static,
    {
        self.action(
            "pausing",
            |container| async move { container.pause().await },
            op,
        );
    }

    pub(crate) fn resume<F>(&self, op: F)
    where
        F: FnOnce(podman::Result<()>) + 'static,
    {
        self.action(
            "resuming",
            |container| async move { container.unpause().await },
            op,
        );
    }

    pub(crate) fn rename<F>(&self, new_name: String, op: F)
    where
        F: FnOnce(podman::Result<()>) + 'static,
    {
        self.action(
            "renaming",
            |container| async move { container.rename(new_name).await },
            op,
        );
    }

    pub(crate) fn commit<F>(&self, op: F)
    where
        F: FnOnce(podman::Result<()>) + 'static,
    {
        self.action(
            "committing",
            |container| async move { container.commit(&Default::default()).await },
            op,
        );
    }

    pub(crate) fn delete<F>(&self, force: bool, op: F)
    where
        F: FnOnce(podman::Result<()>) + 'static,
    {
        self.action(
            if force { "force deleting" } else { "deleting" },
            move |container| async move {
                container
                    .delete(
                        &podman::opts::ContainerDeleteOpts::builder()
                            .force(force)
                            .build(),
                    )
                    .await
            },
            op,
        );
    }

    pub(super) fn on_deleted(&self) {
        if let Some(pod) = self.pod() {
            pod.inspect_and_update();
        }
        self.emit_by_name::<()>("deleted", &[]);
    }
    pub(crate) fn connect_deleted<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_local("deleted", true, move |values| {
            f(&values[0].get::<Self>().unwrap());

            None
        })
    }

    pub(crate) fn api(&self) -> Option<podman::api::Container> {
        self.container_list()
            .unwrap()
            .client()
            .map(|client| podman::api::Container::new(client.podman().deref().clone(), self.id()))
    }
}

fn status(s: Option<&str>) -> Status {
    s.map(|s| match Status::from_str(s) {
        Ok(status) => status,
        Err(status) => {
            log::warn!("Unknown container status: {s}");
            status
        }
    })
    .unwrap_or_default()
}

fn health_status(s: Option<&str>) -> HealthStatus {
    s.map(|s| match HealthStatus::from_str(s) {
        Ok(status) => status,
        Err(status) => {
            log::warn!("Unknown container health status: {s}");
            status
        }
    })
    .unwrap_or_default()
}
