;; round evaluates its argument once and rejects a STRING.
;; Level: boundary
;; Covers: round
(deffunction mark (?digit ?value) (printout t "mark " ?digit crlf) ?value)
(defrule numbers (declare (salience 10)) =>
  (bind ?half (round (mark 1 2.5)))
  (bind ?large (round (mark 1 9007199254740993)))
  (printout t ?half " " ?large crlf))
(defrule probe =>
  (bind ?result (round (mark 2 "2.5")))
  (printout t "not reached " ?result crlf))
