(defmodule A (export ?ALL))
(deftemplate A::foo)
(defmodule B (export ?ALL))
(deftemplate B::foo)
(defmodule C
   (import A ?ALL)
   (import B ?ALL))
(defrule C::bar (foo) =>)
