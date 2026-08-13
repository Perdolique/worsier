import{type ComponentProps,createElement as h}from'react';

const View = ({ title }: { title: string }) => <section><h1>{title}</h1></section>;

const identity = <Value,>(value: Value) => value;

const props = {
  title: identity('Worsier'),
};
