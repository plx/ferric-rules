;; A field with 65 mixed alternatives matches once per fact, including under not.
;; Level: boundary
;; Covers: patterns, field-disjunction, not, assert, retract, salience
(deffacts seed (sym a) (phase 1))
(defrule wide
  (sym ?s&~a|b1|b2|b3|b4|b5|b6|b7|b8|b9|b10|b11|b12|b13|b14|b15|b16|b17|b18|b19|b20|b21|b22|b23|b24|b25|b26|b27|b28|b29|b30|b31|b32|b33|b34|b35|b36|b37|b38|b39|b40|b41|b42|b43|b44|b45|b46|b47|b48|b49|b50|b51|b52|b53|b54|b55|b56|b57|b58|b59|b60|b61|b62|b63|b64)
  => (printout t "wide " ?s crlf))
(defrule none
  (phase ?p)
  (not (sym ~a|b1|b2|b3|b4|b5|b6|b7|b8|b9|b10|b11|b12|b13|b14|b15|b16|b17|b18|b19|b20|b21|b22|b23|b24|b25|b26|b27|b28|b29|b30|b31|b32|b33|b34|b35|b36|b37|b38|b39|b40|b41|b42|b43|b44|b45|b46|b47|b48|b49|b50|b51|b52|b53|b54|b55|b56|b57|b58|b59|b60|b61|b62|b63|b64))
  => (printout t "none " ?p crlf))
(defrule add-overlap
  (declare (salience -5))
  ?phase <- (phase 1)
  => (retract ?phase) (assert (sym b7) (phase 2)) (printout t "added b7" crlf))
(defrule remove-overlap
  (declare (salience -5))
  ?phase <- (phase 2)
  ?s <- (sym b7)
  => (retract ?phase ?s) (assert (phase 3)) (printout t "removed b7" crlf))
(defrule add-negative
  (declare (salience -5))
  ?phase <- (phase 3)
  => (retract ?phase) (assert (sym z) (phase 4)) (printout t "added z" crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
