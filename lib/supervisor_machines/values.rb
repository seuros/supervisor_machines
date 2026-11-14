# frozen_string_literal: true

# Every value mirrors the core type of the same name, field for field, and is
# validated through the core when it is built, by hand or by the gem: wrong
# type is a TypeError, a value the core's constructor rejects a RangeError.
# Every instant and span is Integer nanoseconds.
module SupervisorMachines
  # The core's `Intensity`: `restarts` tolerated inside `window` nanoseconds.
  # Like `Intensity::new`, the window must be longer than zero (RangeError);
  # `restarts` is a nonnegative Integer that fits a u32.
  Intensity = Data.define(:restarts, :window) do
    # The core's `Intensity::default`: OTP's one restart in five seconds.
    def self.default = new(DEFAULT_RESTARTS, DEFAULT_WINDOW)

    def initialize(restarts:, window:)
      restarts, window = SupervisorMachines.__send__(:__intensity, restarts, window)
      super
    end
  end

  # The core's `Span`: the inclusive range of child indices `first..last`.
  # Like `Span::new`, `first` past `last` is a RangeError; it always covers at
  # least one child.
  Span = Data.define(:first, :last) do
    # The core's `Span::single`: the one child at `index`.
    def self.single(index) = new(index, index)

    def initialize(first:, last:)
      first, last = SupervisorMachines.__send__(:__span, first, last)
      super
    end

    # Children in the span, the core's `len`. Always at least one.
    def size = last - first + 1

    # Never true: a span always covers the child that died.
    def empty? = false

    # Whether the child at `index` is in the span.
    def contains?(index) = index.is_a?(Integer) && first <= index && index <= last
  end

  # The core's `Decision::Restart(Span)`: restart `span`, in index order. The
  # other decisions carry nothing and are symbols (:ignore, :escalate,
  # :already_escalated).
  Restart = Data.define(:span) do
    def initialize(span:)
      raise TypeError, "expected a SupervisorMachines::Span, got #{span.inspect}" unless span.instance_of?(Span)

      # Rebuilt through the core's check: `Marshal.load` skips `initialize`.
      super(span: Span.new(span.first, span.last))
    end
  end

  # The values are final. `initialize` is where each validates, and a subclass
  # could override it, or `allocate` build an instance that never ran it:
  # subclassing raises TypeError (as subclassing Class does) and `allocate`
  # is gone. `new`, keywords, `with` and `dup` all still run the check.
  [Intensity, Span, Restart].each do |value|
    value.singleton_class.undef_method(:allocate)
    value.define_singleton_method(:inherited) { |_| raise TypeError, "#{value} cannot be subclassed" }
  end
end
