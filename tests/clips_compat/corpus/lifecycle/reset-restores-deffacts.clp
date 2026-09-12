;; Level: interaction
;; Covers: reset, deffacts, retract
;; Resets: 2
(deffacts seed (value 7))
(defrule probe ?f <- (value ?x) => (printout t ?x crlf) (retract ?f))
