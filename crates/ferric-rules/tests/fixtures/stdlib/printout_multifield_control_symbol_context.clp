
(defrule exercise =>
(printout t "top:[" crlf tab vtab ff "]" crlf)
(printout t "dynamic:[" (sym-cat "crlf") (sym-cat "tab") (sym-cat "vtab") (sym-cat "ff") "]" crlf)
(printout t "fields:" (create$ crlf tab vtab ff "crlf" "tab" "vtab" "ff") crlf)
)
