(defrule probe =>
 (printout t (str-index "" "é🙂") ":" (str-index "🙂" "é🙂") ":"
  (str-index "" "é") ":" (str-index "́" "é") crlf))
