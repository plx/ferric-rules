;; Output preceding an invalid argument remains visible without a trailing newline.
;; Level: interaction
;; Covers: format, printout, deffunction, evaluation-error
(deffacts seed (value abc))
(defrule probe (value ?x) => (printout t "a " (+ 1 ?x)))
