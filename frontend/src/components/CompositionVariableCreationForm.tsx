import { createSignal } from "solid-js";
import { t } from "~/lib/i18n";

export type CompositionVariableCreationKind = "constant" | "input" | "query";

export function CompositionVariableCreationForm(props: {
  existingNames: readonly string[];
  suggestedName?: string;
  onCreate: (name: string) => boolean;
  onCancel: () => void;
  focusAfterCreate?: () => void;
}) {
  const [rawName, setRawName] = createSignal(props.suggestedName ?? "");
  const [creationFailed, setCreationFailed] = createSignal(false);
  let nameInput: HTMLInputElement | undefined;
  const name = () => rawName().trim();
  const duplicate = () => !!name() && props.existingNames.includes(name());
  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    if (!name() || duplicate()) return;
    if (props.onCreate(name())) {
      setCreationFailed(false);
      nameInput?.blur();
      queueMicrotask(() => props.focusAfterCreate?.());
    } else {
      setCreationFailed(true);
    }
  };

  return (
    <form
      class="compositionVariableCreationForm"
      onSubmit={submit}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          props.onCancel();
        }
      }}
    >
      <label>
        <span>{t("composition.studioVariableId")}</span>
        <input
          ref={(element) => {
            nameInput = element;
            queueMicrotask(() => element.focus());
          }}
          class="ui-input compositionVariableCreationName"
          required
          aria-required="true"
          aria-label={t("composition.studioVariableId")}
          aria-invalid={duplicate() ? "true" : undefined}
          aria-describedby={duplicate()
            ? "composition-variable-name-error"
            : undefined}
          value={rawName()}
          onInput={(event) => {
            setRawName(event.currentTarget.value);
            setCreationFailed(false);
          }}
        />
      </label>
      <div class="compositionVariableCreationActions">
        <button type="submit" disabled={!name() || duplicate()}>
          {t("composition.studioCreateVariable")}
        </button>
        <button type="button" onClick={props.onCancel}>
          {t("common.cancel")}
        </button>
      </div>
      {duplicate() && (
        <p
          id="composition-variable-name-error"
          class="ui-text-danger"
          role="alert"
        >
          {t("composition.studioVariableNameDuplicate")}
        </p>
      )}
      {creationFailed() && !duplicate() && (
        <p class="ui-text-danger" role="alert">
          {t("composition.studioVariableCreationFailed")}
        </p>
      )}
    </form>
  );
}
