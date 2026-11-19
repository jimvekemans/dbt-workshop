use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use thiserror::Error;

use crate::{Config, CoreError, Environment, ProjectLayout};

/// A service could not be created (or was never registered).
#[derive(Debug, Clone, Error)]
#[error("{service}: {message}")]
pub struct ServiceError {
    pub service: String,
    pub message: String,
}

impl ServiceError {
    /// Creates an error for `service` with `message`.
    pub fn new(service: impl Into<String>, message: impl fmt::Display) -> Self {
        Self {
            service: service.into(),
            message: message.to_string(),
        }
    }

    fn of<T>(message: impl fmt::Display) -> Self {
        Self::new(short_type_name::<T>(), message)
    }
}

type AnyValue = Arc<dyn Any + Send + Sync>;
type Factory = Box<dyn Fn(&Context) -> Result<AnyValue, ServiceError> + Send + Sync>;

enum SlotState {
    Empty,
    Initialising,
    Ready(AnyValue),
    Failed(ServiceError),
}

struct Slot {
    name: String,
    factory: Factory,
    state: Mutex<SlotState>,
}

/// What every module command receives: the environment, the project, the configuration and lazily created
/// shared services.
///
/// Services are keyed by type, usually a trait object handle such as `Arc<dyn SecretProvider>`. The composition
/// root registers a factory per service with [`ContextBuilder::provide`]; the first [`Context::service`] call
/// runs it (factories may ask the context for other services) and every later call returns the same value. A
/// failure is remembered too, so a broken Vault is reported once, not retried by every module.
pub struct Context {
    environment: Environment,
    project: Option<ProjectLayout>,
    cwd: PathBuf,
    config: Config,
    in_ci: bool,
    slots: HashMap<TypeId, Slot>,
}

impl fmt::Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Context")
            .field("environment", &self.environment)
            .field("project", &self.project)
            .field("cwd", &self.cwd)
            .field("services", &self.slots.len())
            .finish_non_exhaustive()
    }
}

impl Context {
    /// Starts a context on `config`: the default environment, no project, the current directory and not in CI.
    pub fn builder(config: Config) -> ContextBuilder {
        ContextBuilder {
            context: Self {
                environment: config.core.default_environment,
                project: None,
                cwd: PathBuf::from("."),
                config,
                in_ci: false,
                slots: HashMap::new(),
            },
        }
    }

    /// The environment commands run against.
    pub const fn environment(&self) -> Environment {
        self.environment
    }

    /// The merged configuration.
    pub const fn config(&self) -> &Config {
        &self.config
    }

    /// The working directory.
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Whether this runs in GitLab CI (`CI_PROJECT_DIR` is set).
    pub const fn in_ci(&self) -> bool {
        self.in_ci
    }

    /// The dbt project, or an error naming where it was looked for.
    ///
    /// # Errors
    ///
    /// [`CoreError::NoProject`] when no dbt project was found at or above the working directory.
    pub fn project(&self) -> Result<&ProjectLayout, CoreError> {
        self.project
            .as_ref()
            .ok_or_else(|| CoreError::NoProject(self.cwd.clone()))
    }

    /// The dbt project, if one was found.
    pub const fn project_opt(&self) -> Option<&ProjectLayout> {
        self.project.as_ref()
    }

    /// The directory with the organisation's CA certificates.
    pub fn assets_dir(&self) -> PathBuf {
        self.config
            .core
            .resolve_assets_dir(self.project.as_ref().map(|p| p.root.as_path()), self.in_ci)
    }

    /// The CA certificate configured for the current environment; `None` when `ca_certificate` is empty (no TLS
    /// and no Vault certificate, as in a local stack).
    pub fn ca_certificate(&self) -> Option<PathBuf> {
        let name = self.config.environment(self.environment).ca_certificate;
        (!name.trim().is_empty()).then(|| self.assets_dir().join(name))
    }

    /// The shared service of type `T`, created on first use.
    ///
    /// # Errors
    ///
    /// [`ServiceError`] when no provider for `T` is registered, when its factory failed (the failure is cached and returned
    /// again on later calls), or when the services depend on each other in a cycle.
    pub fn service<T: Any + Send + Sync + Clone>(&self) -> Result<T, ServiceError> {
        let slot = self.slots.get(&TypeId::of::<T>()).ok_or_else(|| {
            ServiceError::of::<T>("not available in this build or command (no provider registered)")
        })?;
        {
            let mut state = lock(slot);
            match &*state {
                SlotState::Ready(value) => return Ok(downcast::<T>(value)),
                SlotState::Failed(error) => return Err(error.clone()),
                SlotState::Initialising => {
                    return Err(ServiceError::of::<T>("dependency cycle between services"));
                }
                SlotState::Empty => *state = SlotState::Initialising,
            }
        }
        // The lock is released while the factory runs, so it can resolve other services.
        let result = (slot.factory)(self);
        let mut state = lock(slot);
        match result {
            Ok(value) => {
                let typed = downcast::<T>(&value);
                *state = SlotState::Ready(value);
                drop(state);
                crate::milestone::initialized_debug(&slot.name, "service initialized");
                Ok(typed)
            }
            Err(error) => {
                *state = SlotState::Failed(error.clone());
                Err(error)
            }
        }
    }

    /// Whether a provider for `T` is registered (without creating it).
    pub fn has_service<T: Any>(&self) -> bool {
        self.slots.contains_key(&TypeId::of::<T>())
    }
}

fn lock(slot: &Slot) -> std::sync::MutexGuard<'_, SlotState> {
    slot.state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn downcast<T: Any + Clone>(value: &AnyValue) -> T {
    value
        .downcast_ref::<T>()
        .expect("slots are keyed by the type they hold")
        .clone()
}

fn short_type_name<T>() -> String {
    // `alloc::sync::Arc<dyn vdp_secrets::SecretProvider>` becomes `Arc<dyn SecretProvider>`.
    let full = type_name::<T>();
    let mut out = String::with_capacity(full.len());
    let mut word = String::new();
    for c in full.chars().chain(std::iter::once(' ')) {
        if c.is_alphanumeric() || c == '_' || c == ':' {
            word.push(c);
        } else {
            out.push_str(word.rsplit("::").next().unwrap_or(&word));
            word.clear();
            out.push(c);
        }
    }
    out.trim_end().to_owned()
}

fn human_service_name<T>() -> String {
    let short = short_type_name::<T>();
    let ident = short
        .trim_end_matches('>')
        .split(['<', ':', ' '])
        .rfind(|part| !part.is_empty() && *part != "dyn")
        .unwrap_or("service");
    let mut human = String::with_capacity(ident.len() + 4);
    let mut prev_lower_or_digit = false;
    for ch in ident.chars() {
        if ch == '_' {
            if !human.is_empty() && !human.ends_with(' ') {
                human.push(' ');
            }
            prev_lower_or_digit = false;
            continue;
        }
        if ch.is_uppercase() && prev_lower_or_digit {
            human.push(' ');
        }
        for lower in ch.to_lowercase() {
            human.push(lower);
        }
        prev_lower_or_digit = ch.is_lowercase() || ch.is_ascii_digit();
    }
    if human.is_empty() {
        "service".into()
    } else {
        human
    }
}

/// Assembles a [`Context`].
#[derive(Debug)]
pub struct ContextBuilder {
    context: Context,
}

impl ContextBuilder {
    /// Sets the environment.
    #[must_use]
    pub fn environment(mut self, environment: Environment) -> Self {
        self.context.environment = environment;
        self
    }

    /// Sets the dbt project (`None`: no project).
    #[must_use]
    pub fn project(mut self, project: Option<ProjectLayout>) -> Self {
        self.context.project = project;
        self
    }

    /// Sets the working directory.
    #[must_use]
    pub fn cwd(mut self, cwd: PathBuf) -> Self {
        self.context.cwd = cwd;
        self
    }

    /// Sets whether the process runs in CI.
    #[must_use]
    pub fn in_ci(mut self, in_ci: bool) -> Self {
        self.context.in_ci = in_ci;
        self
    }

    /// Registers the factory for service `T` with a human-readable default name, replacing an earlier one.
    #[must_use]
    pub fn provide<T, F>(self, factory: F) -> Self
    where
        T: Any + Send + Sync + Clone,
        F: Fn(&Context) -> Result<T, ServiceError> + Send + Sync + 'static,
    {
        self.provide_named(human_service_name::<T>(), factory)
    }

    /// Registers the factory for service `T` under `name` (reported in init milestones), replacing an earlier one.
    #[must_use]
    pub fn provide_named<T, F>(mut self, name: impl Into<String>, factory: F) -> Self
    where
        T: Any + Send + Sync + Clone,
        F: Fn(&Context) -> Result<T, ServiceError> + Send + Sync + 'static,
    {
        let factory: Factory =
            Box::new(move |ctx| factory(ctx).map(|value| Arc::new(value) as AnyValue));
        self.context.slots.insert(
            TypeId::of::<T>(),
            Slot {
                name: name.into(),
                factory,
                state: Mutex::new(SlotState::Empty),
            },
        );
        self
    }

    /// Registers a ready-made value for service `T` (tests, or values computed up front).
    #[must_use]
    pub fn provide_value<T: Any + Send + Sync + Clone>(self, value: T) -> Self {
        self.provide(move |_| Ok(value.clone()))
    }

    /// Registers a ready-made value for service `T` under `name`.
    #[must_use]
    pub fn provide_value_named<T: Any + Send + Sync + Clone>(
        self,
        name: impl Into<String>,
        value: T,
    ) -> Self {
        self.provide_named(name, move |_| Ok(value.clone()))
    }

    /// Finishes the context.
    pub fn build(self) -> Context {
        self.context
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    trait Greeter: Send + Sync {
        fn greet(&self) -> String;
    }

    struct Hello(String);

    impl Greeter for Hello {
        fn greet(&self) -> String {
            format!("hello {}", self.0)
        }
    }

    #[derive(Clone)]
    struct Name(String);

    #[test]
    fn services_are_created_once_and_can_depend_on_each_other() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let context = Context::builder(Config::default())
            .provide(|_| Ok(Name("vdp".into())))
            .provide(move |ctx| {
                counter.fetch_add(1, Ordering::SeqCst);
                let name = ctx.service::<Name>()?;
                Ok(Arc::new(Hello(name.0)) as Arc<dyn Greeter>)
            })
            .build();
        for _ in 0..3 {
            assert_eq!(
                context.service::<Arc<dyn Greeter>>().unwrap().greet(),
                "hello vdp"
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn missing_and_failing_services_name_themselves() {
        let context = Context::builder(Config::default())
            .provide(|_| Err::<Name, _>(ServiceError::new("vault", "unreachable")))
            .build();
        let missing = context.service::<Arc<dyn Greeter>>().err().unwrap();
        assert!(
            missing.to_string().starts_with("Arc<dyn Greeter>:"),
            "{missing}"
        );
        assert_eq!(
            context.service::<Name>().err().unwrap().to_string(),
            "vault: unreachable"
        );
    }

    #[test]
    fn cycles_are_reported_instead_of_deadlocking() {
        let context = Context::builder(Config::default())
            .provide(Context::service::<Name>)
            .build();
        let error = context.service::<Name>().err().unwrap();
        assert!(error.to_string().contains("cycle"), "{error}");
    }

    #[test]
    fn service_init_emits_structured_initialized_milestone_once() {
        use std::fmt::Debug;

        use tracing::Subscriber;
        use tracing::field::{Field, Visit};
        use tracing_subscriber::layer::{Context as LayerContext, Layer, SubscriberExt as _};

        #[derive(Default)]
        struct FieldVisitor {
            milestone: Option<String>,
            subject: Option<String>,
            message: Option<String>,
        }

        impl Visit for FieldVisitor {
            fn record_str(&mut self, field: &Field, value: &str) {
                match field.name() {
                    "milestone" => self.milestone = Some(value.to_owned()),
                    "subject" => self.subject = Some(value.to_owned()),
                    "message" => self.message = Some(value.to_owned()),
                    _ => {}
                }
            }

            fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
                if field.name() == "message" {
                    self.message = Some(format!("{value:?}"));
                }
            }
        }

        type CapturedMilestones = Arc<Mutex<Vec<(tracing::Level, String, String, String)>>>;
        struct MilestoneLayer(CapturedMilestones);

        impl<S: Subscriber> Layer<S> for MilestoneLayer {
            fn on_event(&self, event: &tracing::Event<'_>, _ctx: LayerContext<'_, S>) {
                let mut visitor = FieldVisitor::default();
                event.record(&mut visitor);
                if let Some(milestone) = visitor.milestone {
                    self.0
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push((
                            *event.metadata().level(),
                            milestone,
                            visitor.subject.unwrap_or_default(),
                            visitor.message.unwrap_or_default(),
                        ));
                }
            }
        }

        let captured = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(MilestoneLayer(Arc::clone(&captured)));
        let context = Context::builder(Config::default())
            .provide(|_| Ok(Name("vdp".into())))
            .provide(|ctx| {
                let name = ctx.service::<Name>()?;
                Ok(Arc::new(Hello(name.0)) as Arc<dyn Greeter>)
            })
            .build();

        tracing::subscriber::with_default(subscriber, || {
            let _ = context.service::<Arc<dyn Greeter>>().unwrap();
            let _ = context.service::<Arc<dyn Greeter>>().unwrap();
        });

        let events = captured
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert_eq!(
            events,
            vec![
                (
                    tracing::Level::DEBUG,
                    "initialized".into(),
                    "name".into(),
                    "service initialized".into()
                ),
                (
                    tracing::Level::DEBUG,
                    "initialized".into(),
                    "greeter".into(),
                    "service initialized".into()
                ),
            ]
        );
    }
}
