import type { CompositionTextStyle } from "~/lib/composition-api";

const styleClass: Record<CompositionTextStyle, string> = {
  title: "flowText flowText--title",
  heading: "flowText flowText--heading",
  body: "flowText flowText--body",
  caption: "flowText flowText--caption",
};

const styleElement = (
  style: CompositionTextStyle,
): "h2" | "p" => (style === "title" || style === "heading" ? "h2" : "p");

/**
 * Fixed-typography text block for the dashboard flow layout. Content and
 * style come from the component declaration; there is no Markdown, HTML,
 * or arbitrary sizing. The route only passes linted enum styles; the body
 * class default below is a defensive guard for out-of-contract values.
 */
export function TextComponent(props: {
  text: string;
  style: CompositionTextStyle;
}) {
  const element = () => styleElement(props.style);
  return (
    <>
      {element() === "h2"
        ? (
          <h2 class={styleClass[props.style] ?? styleClass.body}>
            {props.text}
          </h2>
        )
        : <p class={styleClass[props.style] ?? styleClass.body}>{props.text}
        </p>}
    </>
  );
}
