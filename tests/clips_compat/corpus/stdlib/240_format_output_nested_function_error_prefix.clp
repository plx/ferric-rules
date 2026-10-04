;; An error in nested printout preserves both outer and inner partial output.
;; Level: interaction
;; Covers: format, printout, deffunction, evaluation-error
(deffunction fail (?x) (printout t "inner " (+ 1 ?x)))
(deffacts seed (value abc))
(defrule probe (value ?x) => (printout t "outer " (fail ?x)))
