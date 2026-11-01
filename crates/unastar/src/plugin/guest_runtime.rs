//! Synchronous WASI calls remain on the world owner while Tokio may drive their I/O.

/// Unsupported executors are rejected before executing guest code or imports.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum GuestRuntimeError {
    #[error("synchronous component calls require a multi-thread Tokio runtime")]
    CurrentThreadRuntime,
    #[error("unsupported Tokio runtime for synchronous component calls")]
    UnsupportedRuntime,
    #[error("synchronous component calls require an executor that permits blocking scopes")]
    BlockingScopeUnavailable,
}

/// Permit sync WASI's nested runtime entry without moving borrowed world state.
/// A blocking scope runs inline on the caller's thread; it is not a worker task.
pub(super) fn call<R>(call: impl FnOnce() -> anyhow::Result<R>) -> anyhow::Result<R> {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => match handle.runtime_flavor() {
            tokio::runtime::RuntimeFlavor::MultiThread => {
                // LocalSet disallows blocking even on a multi-thread runtime. Tokio
                // exposes no public predicate for this additional executor state.
                let entered = std::cell::Cell::new(false);
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    tokio::task::block_in_place(|| {
                        entered.set(true);
                        call()
                    })
                }));
                match result {
                    Ok(result) => result,
                    Err(panic) if entered.get() => std::panic::resume_unwind(panic),
                    Err(_) => Err(GuestRuntimeError::BlockingScopeUnavailable.into()),
                }
            }
            tokio::runtime::RuntimeFlavor::CurrentThread => {
                Err(GuestRuntimeError::CurrentThreadRuntime.into())
            }
            _ => Err(GuestRuntimeError::UnsupportedRuntime.into()),
        },
        // Synchronous embedders can use WASI's own ambient runtime.
        Err(_) => call(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};

    #[test]
    fn synchronous_calls_keep_the_original_result_and_error_chain() {
        #[derive(Debug, thiserror::Error)]
        #[error("fixture error")]
        struct FixtureError;
        assert_eq!(call(|| Ok(7)).unwrap(), 7);
        assert!(
            call::<()>(|| Err(FixtureError.into()))
                .unwrap_err()
                .is::<FixtureError>()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn blocking_scope_keeps_non_send_world_data_on_its_owner_thread() {
        let owner = std::thread::current().id();
        let marker = Rc::new(Cell::new(0));
        let mut world = bevy_ecs::world::World::new();
        world.insert_non_send_resource(marker.clone());
        call(|| {
            assert_eq!(std::thread::current().id(), owner);
            world.non_send_resource::<Rc<Cell<i32>>>().set(42);
            Ok(())
        })
        .unwrap();
        assert_eq!(marker.get(), 42);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn local_set_is_rejected_before_guest_entry() {
        let entered = Cell::new(false);
        tokio::task::LocalSet::new()
            .run_until(async {
                let error = call(|| {
                    entered.set(true);
                    Ok(())
                })
                .unwrap_err();
                assert_eq!(
                    error.downcast_ref::<GuestRuntimeError>(),
                    Some(&GuestRuntimeError::BlockingScopeUnavailable)
                );
            })
            .await;
        assert!(!entered.get());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_native_panic_after_entry_is_not_relabelled_as_an_executor_error() {
        let result = std::panic::catch_unwind(|| call::<()>(|| panic!("native fixture panic")));
        assert!(result.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn current_thread_runtime_is_typed_rejection_before_any_guest_work() {
        let entered = Cell::new(false);
        let result = call(|| {
            entered.set(true);
            Ok(())
        });
        assert_eq!(
            result.unwrap_err().downcast_ref::<GuestRuntimeError>(),
            Some(&GuestRuntimeError::CurrentThreadRuntime)
        );
        assert!(!entered.get());
    }
}
