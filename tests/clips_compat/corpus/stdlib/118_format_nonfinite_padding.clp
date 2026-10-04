(defrule probe =>
(printout t "inf:[" (format nil "%f|%08f|%-08f|%08e|%08g" 1e309 1e309 1e309 1e309 1e309) "]" crlf)
(printout t "negative:[" (format nil "%08f|%08e|%08g" -1e309 -1e309 -1e309) "]" crlf)
;; NaN signs from libm/printf vary by platform. Check the displayed sign.
(bind ?nan (sin 1e309))
(bind ?negative (eq (str-cat ?nan) "-nan.0"))
(bind ?plain (if ?negative then "-nan" else "nan"))
(bind ?right (if ?negative then "    -nan" else "     nan"))
(bind ?left (if ?negative then "-nan    " else "nan     "))
(printout t "nan:["
  (eq (format nil "%f" ?nan) ?plain) "|"
  (eq (format nil "%08f" ?nan) ?right) "|"
  (eq (format nil "%-08f" ?nan) ?left) "|"
  (eq (format nil "%08e" ?nan) ?right) "|"
  (eq (format nil "%08g" ?nan) ?right) "]" crlf)
)
