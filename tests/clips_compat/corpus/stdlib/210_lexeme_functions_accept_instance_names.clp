;; The string functions read an instance name as its unbracketed name;
;; upcase and lowcase keep the INSTANCE-NAME type.
;; Level: boundary
;; Covers: upcase, lowcase, str-length, sub-string, str-index, str-compare
(defrule probe =>
  (printout t (upcase [abc]) " " (instance-namep (upcase [abc])) crlf)
  (printout t (lowcase [ABC]) " " (instance-namep (lowcase [ABC])) crlf)
  (printout t (str-length [abc]) " " (str-length [a-b]) crlf)
  (printout t (sub-string 1 2 [abc]) " " (stringp (sub-string 1 2 [abc])) crlf)
  (printout t (str-index b [abc]) " " (str-index [b] abc) " " (str-index [z] abc) crlf)
  (printout t (str-compare [abc] abc) " " (str-compare [b] [a]) " " (str-compare [a] "b") crlf)
  (printout t (str-cat [abc] x) " " (sym-cat [abc] x) crlf))
