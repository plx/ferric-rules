;; Exists conditions on the same pattern are satisfied by one fact in the order
;; CLIPS visits their joins, newest rule first, so the oldest rule fires first.
;; Level: boundary
;; Covers: patterns, exists, salience, agenda
(defrule r0 (a) (exists (item ?)) => (printout t "r0" crlf))
(defrule r1 (b) (exists (item ?)) => (printout t "r1" crlf))
(defrule r2 (c) (exists (item ?)) => (printout t "r2" crlf))
(defrule seed (declare (salience 10)) => (assert (a) (b) (c)) (assert (item 1)))
