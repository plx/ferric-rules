;; halt stops the run only after the current RHS finishes: query bodies,
;; queries and loops all continue after it.
;; Level: interaction
;; Covers: halt, do-for-all-facts, while, loop-for-count
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)))
(defrule probe (declare (salience 10)) =>
  (do-for-all-facts ((?f item)) TRUE
    (printout t "before:" ?f:value crlf)
    (halt)
    (printout t "inside-after" crlf))
  (bind ?i 0)
  (while (< ?i 3)
    (bind ?i (+ ?i 1))
    (printout t "w" ?i crlf)
    (halt)
    (printout t "w-after" crlf))
  (loop-for-count (?j 2) (printout t "l" ?j crlf) (halt) (printout t "l-after" crlf))
  (printout t "outside-after" crlf))
(defrule never => (printout t "not reached" crlf))
