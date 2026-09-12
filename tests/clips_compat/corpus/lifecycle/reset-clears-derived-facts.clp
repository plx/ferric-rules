;; Level: interaction
;; Covers: reset, assert, not
;; Resets: 2
(deffacts seed (seed))
(defrule probe (seed) (not (derived)) => (printout t "derive" crlf) (assert (derived)))
