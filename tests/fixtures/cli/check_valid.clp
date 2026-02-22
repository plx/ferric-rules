(deftemplate person (slot name) (slot age))
(defrule greet-adults
  (person (name ?n) (age ?a&:(> ?a 18)))
  =>
  (printout t "Hello, " ?n crlf))
