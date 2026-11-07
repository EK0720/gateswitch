// ⓘ with the long explanation as tooltip (design brief: no description lines).
export function Info({ text }: { text: string }) {
  return (
    <i className="info" title={text} aria-label={text} role="img">i</i>
  );
}
