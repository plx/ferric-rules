(deffunction measure (?text) (str-length ?text))
(defrule probe =>
  (printout t (measure abc) ":" (measure "abc") ":" (measure "") crlf))
