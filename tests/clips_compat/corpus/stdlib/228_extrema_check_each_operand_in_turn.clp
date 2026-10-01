;; min and max check each operand as they evaluate it; NaN comparisons keep the first operand.
;; Level: boundary
;; Covers: min, max
(deffunction mark (?digit ?value) (printout t "mark " ?digit crlf) ?value)
(defrule nan (declare (salience 10)) =>
  (printout t (floatp (min (sin 1.0e309) 1)) " " (integerp (min 1 (sin 1.0e309))) " " (floatp (max (sin 1.0e309) 1)) " " (integerp (max 1 (sin 1.0e309))) crlf))
(defrule probe =>
  (bind ?result (min (mark 1 wrong) (mark 2 4) (mark 3 8)))
  (printout t "not reached " ?result crlf))
