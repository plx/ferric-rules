(defmodule MAIN (export ?ALL))
(deftemplate block)
(deffacts seed (block))
(defmodule WATCH (import MAIN ?ALL))
(defrule WATCH::absent (declare (auto-focus TRUE)) (not (block)) => (printout t absent crlf))
(defrule MAIN::release ?b <- (block) =>
  (retract ?b) (printout t (get-focus-stack) crlf))
