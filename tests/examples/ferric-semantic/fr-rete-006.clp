; FR-RETE-006: complete tuples inside exists collapse to one Boolean support.
(deffacts startup
  (phase zero))

(defrule zero-support
  (declare (salience 100))
  ?phase <- (phase zero)
  =>
  (printout t "zero" crlf)
  (retract ?phase)
  (assert (a one))
  (assert (b one)))

(defrule existential-match
  (declare (salience 80))
  (exists (a ?value) (b ?value))
  =>
  (printout t "one" crlf)
  (assert (a two))
  (assert (b two))
  (assert (phase two)))

(defrule two-supports
  (declare (salience 60))
  ?phase <- (phase two)
  ?first-a <- (a one)
  ?first-b <- (b one)
  =>
  (printout t "two" crlf)
  (retract ?phase ?first-a ?first-b)
  (assert (phase partial)))

(defrule partial-support-retract
  (declare (salience 40))
  ?phase <- (phase partial)
  ?last-a <- (a two)
  ?last-b <- (b two)
  =>
  (printout t "partial" crlf)
  (retract ?phase ?last-a ?last-b)
  (assert (phase zero-again)))

(defrule zero-support-again
  (declare (salience 20))
  (phase zero-again)
  =>
  (printout t "zero-again" crlf))
