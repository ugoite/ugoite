import {
  createResource as createSolidResource,
  type Resource,
} from "solid-js";

/**
 * Solid Resources throw their stored error whenever the accessor is read.
 * That is useful with Suspense, but it also bypasses local `resource.error`
 * recovery UI when a sibling computation reads the value. Keep the Resource
 * state and actions intact while making an errored value read as unavailable.
 */
export function recoverResource<T>(resource: Resource<T>): Resource<T> {
  return new Proxy(resource, {
    apply(target, thisArgument, argumentsList) {
      if (target.state === "errored") return undefined;
      return Reflect.apply(target, thisArgument, argumentsList);
    },
  }) as Resource<T>;
}

const createRecoverableResource = ((...args: unknown[]) => {
  const create = createSolidResource as unknown as (
    ...resourceArgs: unknown[]
  ) => [Resource<unknown>, unknown];
  const [resource, actions] = create(...args);
  return [recoverResource(resource), actions];
}) as typeof createSolidResource;

// Drop-in replacement so every API-backed Resource follows the same error
// policy without changing its loading, error, mutate, or refetch interface.
export { createRecoverableResource as createResource };
