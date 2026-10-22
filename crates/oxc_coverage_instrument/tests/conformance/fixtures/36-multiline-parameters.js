function configure({
  width = 1,
  height = 2,
}) {
  return width * height;
}

const area = (
  width,
  height,
) => width * height;

const shapes = {
  scale(
    factor,
    offset,
  ) {
    return factor + offset;
  },
};

globalThis.multilineParameterResult = [configure({}), area(2, 3), shapes.scale(1, 2)];
