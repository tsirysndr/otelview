export function KeyValueTable({ data }: { data: Record<string, unknown> }) {
  const entries = Object.entries(data ?? {});
  if (entries.length === 0) {
    return <p className="px-1 py-2 text-xs text-default-400">none</p>;
  }
  return (
    <table className="w-full table-fixed border-collapse text-xs">
      <tbody>
        {entries.map(([k, v]) => (
          <tr key={k} className="border-b border-divider/50 align-top">
            <td className="w-[45%] break-all py-1 pr-2 text-default-500">{k}</td>
            <td className="break-all py-1 text-foreground">
              {typeof v === "string" ? v : JSON.stringify(v)}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
