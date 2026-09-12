;; get-focus observes a focus change within the same RHS.
;; Level: interaction
;; Covers: modules, focus-rhs-observation
(defmodule WORK)
(defrule MAIN::probe =>
    (focus WORK)
    (printout t (get-focus) ":" (get-focus-stack) crlf))
