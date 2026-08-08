; FR-RETE-004: test CEs are evaluated when a match forms, not when it fires.
(defglobal ?*gate* = FALSE)

(deffacts startup
  (historical)
  (phase open))

(defrule historical-match
  (historical)
  (test (eq ?*gate* TRUE))
  =>
  (printout t "historical-fired" crlf))

(defrule open-gate
  (declare (salience 100))
  ?phase <- (phase open)
  =>
  (printout t "open" crlf)
  (retract ?phase)
  (bind ?*gate* TRUE)
  (assert (fresh)))

(defrule fresh-match
  (declare (salience 80))
  ?fresh <- (fresh)
  (test (eq ?*gate* TRUE))
  =>
  (printout t "fresh" crlf)
  (retract ?fresh)
  (assert (positive 1))
  (assert (phase retract)))

(defrule retract-before-fire
  (declare (salience 100))
  ?phase <- (phase retract)
  ?positive <- (positive ?value)
  =>
  (printout t "retract" crlf)
  (retract ?phase ?positive)
  (assert (phase reassert)))

(defrule reassert-positive
  (declare (salience 100))
  ?phase <- (phase reassert)
  =>
  (printout t "reassert" crlf)
  (retract ?phase)
  (assert (positive 2)))

(defrule positive-match
  (declare (salience 10))
  (positive ?value)
  (test (> ?value 0))
  =>
  (printout t "positive " ?value crlf))
