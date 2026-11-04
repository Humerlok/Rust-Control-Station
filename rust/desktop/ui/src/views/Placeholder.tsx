export function Placeholder({ title }: { title: string }) {
  return (
    <div className="placeholder">
      <div>
        <h1>{title}</h1>
        <p>Coming in a later phase.</p>
      </div>
    </div>
  );
}
