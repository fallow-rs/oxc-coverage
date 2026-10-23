class Twin {
  #privateMethod(value) {
    if (value === 0) return 10;
    if (value === 1) return 20;
    return 30;
  }
  publicMethod(value) {
    if (value === 0) return 10;
    if (value === 1) return 20;
    return 30;
  }
  callPrivate(value) {
    return this.#privateMethod(value);
  }
}
const twin = new Twin();
twin.callPrivate(0);
twin.publicMethod(0);
