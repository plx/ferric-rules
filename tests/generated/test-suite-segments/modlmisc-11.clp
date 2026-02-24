(defmodule MAIN (export ?ALL))
(deftemplate MAIN::foo)
(defmodule A (import MAIN ?ALL) (export ?ALL))
(defmodule B (import MAIN ?ALL) (export ?ALL))
(defmodule C
   (import A ?ALL)
   (import B ?ALL))
(defrule C::bar (foo) =>)
