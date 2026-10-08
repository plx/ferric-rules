(deftemplate item (multislot values (cardinality 2 3)))
(defrule empty (item (values)) => (printout t "empty" crlf))
(defrule none (not (item (values))) => (printout t "none" crlf))
