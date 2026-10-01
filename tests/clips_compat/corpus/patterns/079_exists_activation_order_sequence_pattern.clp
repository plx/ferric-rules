;; Exists conditions on different patterns of one template, one of them a
;; multislot sequence, are satisfied by one fact newest rule first, whichever
;; pattern each tests.
;; Level: boundary
;; Covers: patterns, exists, not, multislot, deftemplate, agenda
(deftemplate item (slot s) (multislot tags))
(defrule r0 (go) (exists (item (s c))) => (printout t "r0" crlf))
(defrule r1 (go) (exists (item (tags ~b $?))) => (printout t "r1" crlf))
(defrule r2 (go) (exists (item (s ~d))) => (printout t "r2" crlf))
(defrule r3 (go) (exists (item (tags $? a $?))) => (printout t "r3" crlf))
(defrule r4 (go) (not (item (tags $? z $?))) => (printout t "r4" crlf))
(defrule seed (declare (salience 10)) => (assert (go)) (assert (item (s c) (tags a))))
