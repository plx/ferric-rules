(defmodule MAIN (export ?ALL))
(defrule MAIN::bar 
  (declare (auto-focus TRUE))
  =>)
(defmodule A (import MAIN ?ALL))
(defrule A::foo 
  (declare (auto-focus TRUE))
  =>)
