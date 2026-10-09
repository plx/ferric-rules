;; An explicit NCC with a test CE evaluates a nonlinear check at match time.
;; Level: interaction
;; Covers: patterns, not, and, ncc, test, join, assert, retract, salience
(deffacts seed (anchor 2) (data 3) (phase 1))
(defrule clear
  (anchor ?min)
  (not (and (data ?x)
            (test (> (* ?x ?x) (* ?min ?min)))))
  => (printout t "clear " ?min crlf))
;; A candidate whose square is not larger does not block.
(defrule add-small
  (declare (salience -5))
  ?phase <- (phase 1)
  => (retract ?phase) (assert (data 1) (phase 2)) (printout t "added data 1" crlf))
;; Removing the final blocker creates an activation.
(defrule remove-blocker
  (declare (salience -5))
  ?phase <- (phase 2)
  ?data <- (data 3)
  => (retract ?phase ?data) (assert (phase 3)) (printout t "removed data 3" crlf))
;; A new blocker asserted with the removal cancels the activation before it fires.
(defrule swap-blocker
  (declare (salience -5))
  ?phase <- (phase 3)
  => (retract ?phase) (assert (data 4) (phase 4)) (printout t "added data 4" crlf))
(defrule swap-again
  (declare (salience -5))
  ?phase <- (phase 4)
  ?data <- (data 4)
  => (retract ?phase ?data) (assert (data 5) (phase 5)) (printout t "replaced data 4 with data 5" crlf))
;; Removing the replacement blocker lets the rule fire again.
(defrule remove-last
  (declare (salience -5))
  ?phase <- (phase 5)
  ?data <- (data 5)
  => (retract ?phase ?data) (printout t "removed data 5" crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
