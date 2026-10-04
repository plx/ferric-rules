;; An empty computed multifield contributes no ordered or multislot fields.
;; Level: interaction
;; Covers: assertion-expression, deffacts, deftemplate, create$, length$
(deftemplate item (multislot empty) (multislot one))
(deffacts seed (row before (create$) after) (empty (create$))
  (item (empty (create$)) (one (create$ 7))))
(defrule show (row $?row) (empty $?empty) (item (empty $?slot) (one $?one))
  => (printout t ?row " " (length$ ?empty) " " (length$ ?slot) " " ?one crlf))
