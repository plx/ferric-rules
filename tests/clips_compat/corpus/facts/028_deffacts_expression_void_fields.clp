;; Void seed results are omitted from ordered fields and multislots.
;; Level: interaction
;; Covers: assertion-expression, deffacts, printout
(deftemplate item (slot n) (multislot tags))
(deffacts seed
  (row before (printout nil "x") after)
  (item (tags p (printout nil "y") q)))
(defrule show-row (row $?fields) => (printout t "row " (length$ ?fields) " " ?fields crlf))
(defrule show-item (item (tags $?tags)) => (printout t "tags " (length$ ?tags) " " ?tags crlf))
