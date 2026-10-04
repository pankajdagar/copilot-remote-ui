import { useUi } from "../store";

export function SessionSearch() {
  const search = useUi((state) => state.search);
  const setSearch = useUi((state) => state.setSearch);
  return (
    <label className="block">
      <span className="sr-only">Search sessions</span>
      <input
        className="field w-full"
        type="search"
        placeholder="Search sessions..."
        value={search}
        onChange={(event) => setSearch(event.target.value)}
      />
    </label>
  );
}
