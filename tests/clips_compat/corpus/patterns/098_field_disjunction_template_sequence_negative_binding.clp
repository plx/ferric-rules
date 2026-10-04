;; An overlapping negative alternative binds each multislot element once.
;; Level: interaction
;; Covers: patterns, field-disjunction, deftemplate, multislot, not, variable-binding
(deftemplate item (multislot tags))
(deffacts seed (item (tags a b c)))
(defrule match (item (tags $? ?t&~a|b $?)) => (printout t "P " ?t crlf))
(defrule complete (declare (salience -10)) => (printout t "done" crlf))
