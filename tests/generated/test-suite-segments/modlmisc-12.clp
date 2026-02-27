(defmodule A (export ?ALL))
(deftemplate A::foo)
(defmodule B (import A ?ALL))
(deftemplate B::foo)
