interface Props { title: string; items: string[]; children?: unknown; }
const Component = ({ title, items, ...props }: Props) => <><section id='root' data-title={title}
  {...props}>hello <h1>{title}</h1>{items
  .map(
  (item) => <Widget.Item key={item} nested={<span>{item}</span>} fragment={<>{item}</>} />
)}</section><svg:path xml:lang='en' />{...items}{ /* comment */ }</>;
const Generic = <T,>({ value }: { value: T }) => <this.View value={value}></this.View>;
