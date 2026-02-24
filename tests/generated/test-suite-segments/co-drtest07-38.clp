(deffacts start (result # 0 1 0))
(defrule print-responses
   (result $?input # $?response)
   =>
   (while (neq ?response (create$)) do
      (nth 1 ?response)
      (bind ?response (create$))))
