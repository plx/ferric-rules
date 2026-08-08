; FR-RETE-008-BREADTH: P is created first; N is destroyed and then recreated.
; Under breadth, the older P activation must fire before newly recreated N.
(deffacts startup
  (start))

(defrule establish-order
  (declare (salience 100))
  ?start <- (start)
  =>
  (retract ?start)
  (assert (positive-ready))
  (assert (blocker))
  (assert (release-blocker)))

(defrule release-negative
  (declare (salience 90))
  ?release <- (release-blocker)
  ?blocker <- (blocker)
  =>
  (retract ?release ?blocker))

(defrule positive-activation
  (positive-ready)
  =>
  (printout t "P" crlf)
  (assert (result P)))

(defrule recreated-negative-activation
  (not (blocker))
  =>
  (printout t "N" crlf)
  (assert (result N)))
