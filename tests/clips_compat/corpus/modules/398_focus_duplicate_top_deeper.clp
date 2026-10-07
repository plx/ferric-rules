(defmodule A)
(defmodule B)
(defrule MAIN::start =>
  (focus A A) (printout t (get-focus-stack) crlf)
  (focus B) (focus A) (printout t (get-focus-stack) crlf))
