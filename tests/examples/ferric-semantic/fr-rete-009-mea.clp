; FR-RETE-009-MEA: two equal-salience pairs distinguish CLIPS's MEA ordering.
; The first has different first-pattern recency keys; the second holds that key
; equal and exercises MEA's canonical LEX fallback.
(deffacts startup
  (t1)
  (t2)
  (t3)
  (t4)
  (t5))

(defrule lex-X
  (declare (salience 20))
  (t1)
  (t4)
  =>
  (printout t "LX" crlf)
  (assert (result LX)))

(defrule lex-Y
  (declare (salience 20))
  (t3)
  (t2)
  =>
  (printout t "LY" crlf)
  (assert (result LY)))

(defrule mea-X
  (declare (salience 10))
  (t1)
  (t2)
  (t5)
  =>
  (printout t "MX" crlf)
  (assert (result MX)))

(defrule mea-Y
  (declare (salience 10))
  (t1)
  (t4)
  (t3)
  =>
  (printout t "MY" crlf)
  (assert (result MY)))
