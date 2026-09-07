;; Reset gives MAIN focus even if another module is declared last.
;; Level: interaction
;; Covers: modules, focus-default
(defrule MAIN::probe => (printout t (get-focus) ":" (get-focus-stack) crlf))
(defmodule IDLE)
