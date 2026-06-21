type AppLinkProps = Record<string, unknown> & {
  href: string;
  children?: unknown;
};

const toClassList = (classList: unknown): string => {
  if (!classList || typeof classList !== "object") {
    return "";
  }
  return Object.entries(classList as Record<string, unknown>)
    .filter(([, enabled]) => Boolean(enabled))
    .map(([className]) => className)
    .join(" ");
};

export default function AppLink(props: AppLinkProps) {
  const { href, children, class: className, classList, ...rest } = props;
  const mergedClass = [className, toClassList(classList)].filter(Boolean).join(
    " ",
  );
  return (
    <a href={href} class={mergedClass || undefined} {...(rest as never)}>
      {children as never}
    </a>
  );
}
