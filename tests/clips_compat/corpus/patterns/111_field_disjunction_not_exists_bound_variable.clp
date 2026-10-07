;; A negated existential tests the whole correlated disjunction as its support changes.
;; Level: interaction
;; Covers: patterns, field-disjunction, not, exists, join, assert, retract, salience
(deffacts seed (key a) (key b) (key c) (item b) (phase 1))
(defrule none (key ?k) (not (exists (item ?k|99)))
  => (printout t "none " ?k crlf))
;; A witness for an already-unsupported key creates no new activation.
(defrule add-a
  (declare (salience -5))
  ?phase <- (phase 1)
  => (retract ?phase) (assert (item a) (phase 2)) (printout t "added a" crlf))
(defrule remove-b
  (declare (salience -5))
  ?phase <- (phase 2)
  ?b <- (item b)
  => (retract ?phase ?b) (assert (phase 3)) (printout t "removed b" crlf))
(defrule swap-a-for-c
  (declare (salience -5))
  ?phase <- (phase 3)
  ?a <- (item a)
  => (retract ?phase ?a) (assert (item c) (phase 4)) (printout t "swapped a for c" crlf))
;; The shared literal alternative blocks every key until it is retracted.
(defrule add-shared
  (declare (salience -5))
  ?phase <- (phase 4)
  => (retract ?phase) (assert (item 99) (item a) (phase 5)) (printout t "added 99 a" crlf))
(defrule remove-shared
  (declare (salience -5))
  ?phase <- (phase 5)
  ?shared <- (item 99)
  => (retract ?phase ?shared) (printout t "removed 99" crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
