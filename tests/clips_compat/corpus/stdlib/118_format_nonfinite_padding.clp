(defrule probe =>
(printout t "inf:[" (format nil "%f|%08f|%-08f|%08e|%08g" 1e309 1e309 1e309 1e309 1e309) "]" crlf)
(printout t "negative:[" (format nil "%08f|%08e|%08g" -1e309 -1e309 -1e309) "]" crlf)
(printout t "nan:[" (format nil "%f|%08f|%-08f|%08e|%08g" (sin 1e309) (sin 1e309) (sin 1e309) (sin 1e309) (sin 1e309)) "]" crlf)
)
